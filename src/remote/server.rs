//! The phone server's lifecycle (specs/remote.md §3, §4): bound to one LAN address
//! and following it as the lease changes, a thread per connection, stopped on drop
//! or when the Mac joins another network.
//! Who gets in is the paired devices' book, which outlives the server. Nothing
//! waits on the UI thread.

use std::io::{self, Seek, SeekFrom};
use std::net::{IpAddr, Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tungstenite::protocol::Role;
use tungstenite::WebSocket;

use crate::remote::access::{session_cookie, session_token, Access, AccessAlert, PairingCode};
use crate::remote::address::{current_lan, interfaces, lan_address, Lan};
use crate::remote::awake::KeepAwake;
use crate::remote::devices::{device_name, wall_ms, PairedDevices};
use crate::remote::files::{self, OpenedFile};
use crate::remote::http::{read_head, RequestHead, RequestedRange, Response};
use crate::remote::launch::Launcher;
use crate::remote::network::current_gateway_mac;
use crate::remote::protocol::PageTheme;
use crate::remote::registry::Registry;
use crate::remote::socket::{PhoneSocket, READ_TIMEOUT};
use crate::terminal::activity::now_ms;

const ACCEPT_POLL: Duration = Duration::from_millis(100);
const ADDRESS_CHECK: Duration = Duration::from_secs(2);
const HEAD_TIMEOUT: Duration = Duration::from_secs(5);
/// A phone that stops reading (a suspended Safari tab) must not pin its thread.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

const INDEX_HTML: &str = include_str!("assets/index.html");
const APP_JS: &str = include_str!("assets/app.js");
const APP_CSS: &str = include_str!("assets/app.css");
const UNPAIRED_HTML: &str = include_str!("assets/unpaired.html");

#[derive(Debug)]
pub enum StartError {
    NoLocalNetwork,
    Bind(io::Error),
}

/// Where the server reports a pairing or a suspicious connection.
pub type AlertSink = Arc<dyn Fn(AccessAlert) + Send + Sync>;

/// What the server works with, all outliving it.
pub struct PhoneServices {
    pub registry: Registry,
    pub launcher: Launcher,
    pub devices: PairedDevices,
    pub alert: AlertSink,
}

impl PhoneServices {
    /// Pairings kept in memory only, alerts dropped: tests and the phone preview.
    pub fn unpersisted(registry: Registry, launcher: Launcher) -> Self {
        Self {
            registry,
            launcher,
            devices: PairedDevices::default(),
            alert: Arc::new(|_| {}),
        }
    }
}

pub struct PhoneServer {
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
}

/// Where the network is now, asked with the bound address.
type Locate = Box<dyn Fn(Option<Ipv4Addr>) -> Lan + Send>;

struct Bound {
    ip: Ipv4Addr,
    listener: TcpListener,
}

struct Shared {
    access: Mutex<Access>,
    services: PhoneServices,
    pairing: Mutex<Option<PairingCode>>,
    sockets: Mutex<Vec<LiveSocket>>,
    stopped: AtomicBool,
    /// A socket opened on an address closes once that address is left.
    addresses_left: AtomicU64,
    awake: Mutex<Option<KeepAwake>>,
}

/// An open WebSocket, by the device it serves and the address it comes from.
#[derive(Clone, PartialEq, Eq)]
struct LiveSocket {
    device: String,
    name: String,
    ip: IpAddr,
}

/// The paired device a request's cookie opens.
struct Visitor {
    id: String,
    name: String,
}

impl Shared {
    fn access(&self) -> MutexGuard<'_, Access> {
        self.access
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Where to go on listening as the network moves: nowhere while it is
    /// unsettled; another network stops access.
    fn follow(&self, mut bound: Option<Bound>, lan: Lan) -> Option<Bound> {
        if matches!((&bound, lan), (Some(bound), Lan::Address(ip)) if bound.ip == ip) {
            return bound;
        }
        if bound.take().is_some() {
            self.addresses_left.fetch_add(1, Ordering::Relaxed);
        }
        match lan {
            Lan::Address(ip) => self.rebind(ip),
            Lan::Unsettled => None,
            Lan::Unrecorded => {
                self.stop();
                None
            }
        }
    }

    fn rebind(&self, ip: Ipv4Addr) -> Option<Bound> {
        let bound = bind(ip, &self.services.devices).ok()?;
        *self.access() = Access::new(bound.listener.local_addr().ok()?);
        Some(bound)
    }

    fn addresses_left(&self) -> u64 {
        self.addresses_left.load(Ordering::Relaxed)
    }

    fn sockets(&self) -> MutexGuard<'_, Vec<LiveSocket>> {
        self.sockets
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pairing(&self) -> MutexGuard<'_, Option<PairingCode>> {
        self.pairing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn visitor(&self, head: &RequestHead) -> Option<Visitor> {
        let token = session_token(head.header("cookie"))?;
        self.services.devices.read(|book| {
            book.device(token, wall_ms()).map(|device| Visitor {
                id: device.id.clone(),
                name: device.name.clone(),
            })
        })
    }

    /// Spends the offered code when `candidate` is it.
    fn redeem_pairing(&self, candidate: Option<&str>) -> bool {
        let mut pairing = self.pairing();
        let redeems = matches!(
            (pairing.as_ref(), candidate),
            (Some(code), Some(candidate)) if code.redeems(candidate, now_ms())
        );
        if redeems {
            *pairing = None;
        }
        redeems
    }

    fn is_paired(&self, device: &str) -> bool {
        self.services.devices.read(|book| book.is_paired(device))
    }

    /// Alerts when the device already has a socket open from another address.
    fn open_socket(&self, socket: LiveSocket) {
        let mut sockets = self.sockets();
        let elsewhere = sockets
            .iter()
            .any(|live| live.device == socket.device && live.ip != socket.ip);
        let device = socket.name.clone();
        sockets.push(socket);
        drop(sockets);
        if elsewhere {
            (self.services.alert)(AccessAlert::TwoAddresses { device });
        }
    }

    fn close_socket(&self, socket: &LiveSocket) {
        let mut sockets = self.sockets();
        if let Some(index) = sockets.iter().position(|live| live == socket) {
            sockets.remove(index);
        }
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
    /// On the machine's LAN address, followed across its leases; access stops on
    /// a network no phone was paired on.
    pub fn start(services: PhoneServices) -> Result<Self, StartError> {
        let ip = lan_address(&interfaces()).ok_or(StartError::NoLocalNetwork)?;
        let devices = services.devices.clone();
        let locate = move |bound| current_lan(bound, &devices);
        Self::start_following(ip, locate, services).map_err(StartError::Bind)
    }

    /// On `ip` for good (tests bind the loopback).
    pub fn start_on_address(ip: Ipv4Addr, services: PhoneServices) -> io::Result<Self> {
        Self::start_following(ip, move |_| Lan::Address(ip), services)
    }

    /// From `ip`, then wherever `locate` — asked with the bound address — says
    /// the network is.
    pub fn start_following(
        ip: Ipv4Addr,
        locate: impl Fn(Option<Ipv4Addr>) -> Lan + Send + 'static,
        services: PhoneServices,
    ) -> io::Result<Self> {
        let locate: Locate = Box::new(locate);
        let bound = bind(ip, &services.devices)?;
        let shared = Arc::new(Shared {
            access: Mutex::new(Access::new(bound.listener.local_addr()?)),
            services,
            pairing: Mutex::new(None),
            sockets: Mutex::new(Vec::new()),
            stopped: AtomicBool::new(false),
            addresses_left: AtomicU64::new(0),
            awake: Mutex::new(Some(KeepAwake::begin())),
        });
        let accepting = Arc::clone(&shared);
        let accept = std::thread::Builder::new()
            .name("phone-accept".into())
            .spawn(move || accept_loop(Some(bound), &accepting, locate))?;
        Ok(Self {
            shared,
            accept: Some(accept),
        })
    }

    /// `http://<ip>:<port>` of the address bound last.
    pub fn origin(&self) -> String {
        self.shared.access().origin().to_owned()
    }

    /// Mints a fresh pairing code, replacing the previous one; its URL is what the
    /// QR code encodes.
    pub fn offer_pairing(&self) -> String {
        let code = PairingCode::mint(now_ms());
        let url = self.shared.access().pairing_url(&code);
        *self.shared.pairing() = Some(code);
        url
    }

    /// The offered code's URL while it can still pair: `None` once used or expired.
    pub fn pairing_url(&self) -> Option<String> {
        self.shared
            .pairing()
            .as_ref()
            .filter(|code| code.is_live(now_ms()))
            .map(|code| self.shared.access().pairing_url(code))
    }

    pub fn clients(&self) -> usize {
        self.shared.sockets().len()
    }

    /// The names of the devices with a socket open, each device once.
    pub fn connected_devices(&self) -> Vec<String> {
        let sockets = self.shared.sockets();
        let mut seen = Vec::new();
        let mut names = Vec::new();
        for socket in sockets.iter() {
            if !seen.contains(&socket.device) {
                seen.push(socket.device.clone());
                names.push(socket.name.clone());
            }
        }
        names
    }

    /// `false` once access stopped on its own (another network).
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
fn accept_loop(mut bound: Option<Bound>, shared: &Arc<Shared>, locate: Locate) {
    let mut address_checked = Instant::now();
    while !shared.is_stopped() {
        match bound.as_ref().map(|bound| bound.listener.accept()) {
            Some(Ok((stream, _))) => {
                let serving = Arc::clone(shared);
                let _ = std::thread::Builder::new()
                    .name("phone-connection".into())
                    .spawn(move || serve(stream, &serving));
            }
            _ => std::thread::sleep(ACCEPT_POLL),
        }
        if address_checked.elapsed() >= ADDRESS_CHECK {
            address_checked = Instant::now();
            let lan = locate(bound.as_ref().map(|bound| bound.ip));
            bound = shared.follow(bound, lan);
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
    let visitor = shared.visitor(&head);
    let response = match head.path.as_str() {
        "/pair" if visitor.is_some() => to_agents(),
        "/pair" => return pair(&mut stream, &head, shared),
        "/ws" if head.is_websocket_upgrade() => return upgrade(stream, &head, visitor, shared),
        "/" | "/app.js" | "/app.css" | "/files" | "/file" if visitor.is_none() => unpaired(),
        "/" => return index(&mut stream, &head, shared),
        "/files" => return list_files(&mut stream, &head, shared),
        "/file" => return send_file(&mut stream, &head, shared),
        "/app.js" => Response::new("200 OK", "text/javascript", APP_JS.as_bytes()),
        "/app.css" => Response::new("200 OK", "text/css", APP_CSS.as_bytes()),
        _ => not_found(),
    };
    let _ = response.write_to(&mut stream);
}

fn not_found() -> Response<'static> {
    Response::new("404 Not Found", "text/plain", b"")
}

fn unpaired() -> Response<'static> {
    Response::new(
        "401 Unauthorized",
        "text/html; charset=utf-8",
        UNPAIRED_HTML.as_bytes(),
    )
}

/// A phone already paired that scans a code again: no second device, the code
/// stays offered.
fn to_agents() -> Response<'static> {
    Response::new("303 See Other", "text/plain", b"").with("Location", "/".to_owned())
}

/// Every page load rotates the device's token: a copied cookie is worth one visit.
fn index(stream: &mut TcpStream, head: &RequestHead, shared: &Shared) {
    let rotated = session_token(head.header("cookie")).and_then(|token| {
        shared
            .services
            .devices
            .edit(|book| book.rotate(token, wall_ms()))
    });
    let Some(fresh) = rotated else {
        let _ = unpaired().write_to(stream);
        return;
    };
    let page = index_page(&shared.services.registry.page_theme());
    let _ = Response::new("200 OK", "text/html; charset=utf-8", page.as_bytes())
        .with("Set-Cookie", session_cookie(&fresh))
        .write_to(stream);
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

/// What `path` names inside the worktree of the agent in `pane`.
fn worktree_path(head: &RequestHead, shared: &Shared) -> Option<PathBuf> {
    let pane = head.query_param("pane")?.parse().ok()?;
    let worktree = shared.services.registry.agent_worktree(pane)?;
    let relative = head.decoded_query_param("path").unwrap_or_default();
    files::resolve(&worktree, &relative)
}

fn list_files(stream: &mut TcpStream, head: &RequestHead, shared: &Shared) {
    let listing = worktree_path(head, shared).and_then(|dir| files::list(&dir).ok());
    let Some(listing) = listing else {
        let _ = not_found().write_to(stream);
        return;
    };
    let json = serde_json::to_vec(&listing).expect("a listing serializes");
    let _ = Response::new("200 OK", "application/json", &json).write_to(stream);
}

/// The whole file, or the part a `Range` asks for: iOS plays no media without it.
fn send_file(stream: &mut TcpStream, head: &RequestHead, shared: &Shared) {
    let opened = worktree_path(head, shared).and_then(|path| OpenedFile::open(&path));
    let Some(mut opened) = opened else {
        let _ = not_found().write_to(stream);
        return;
    };
    let len = opened.len;
    let part = match RequestedRange::of(head.header("range"), len) {
        RequestedRange::Whole => None,
        RequestedRange::Part(part) => Some(part),
        RequestedRange::Unsatisfiable => {
            let _ = Response::new("416 Range Not Satisfiable", "text/plain", b"")
                .with("Content-Range", format!("bytes */{len}"))
                .write_to(stream);
            return;
        }
    };
    let status = match part {
        Some(_) => "206 Partial Content",
        None => "200 OK",
    };
    let mut response = Response::new(status, opened.content_type, b"")
        .with("Accept-Ranges", "bytes".to_owned())
        .with("X-Content-Type-Options", "nosniff".to_owned());
    if let Some(part) = &part {
        let range = format!("bytes {}-{}/{len}", part.start, part.end - 1);
        response = response.with("Content-Range", range);
    }
    let sent = part.unwrap_or(0..len);
    if opened.runs_scripts() {
        let sandbox = "sandbox allow-scripts".to_owned();
        response = response.with("Content-Security-Policy", sandbox);
    }
    if opened.file.seek(SeekFrom::Start(sent.start)).is_err() {
        return;
    }
    let _ = response.write_streaming(stream, &mut opened.file, sent.end - sent.start);
}

/// The offered code, spent, becomes a paired device and its session cookie; the
/// code leaves the address bar. The network it paired on is recorded for *Start
/// at launch*.
fn pair(stream: &mut TcpStream, head: &RequestHead, shared: &Shared) {
    if !shared.redeem_pairing(head.query_param("t")) {
        let _ = unpaired().write_to(stream);
        return;
    }
    let name = device_name(head.header("user-agent"));
    let network = current_gateway_mac();
    let token = shared.services.devices.edit(|book| {
        if let Some(mac) = &network {
            book.record_network(mac);
        }
        book.pair(name, wall_ms())
    });
    (shared.services.alert)(AccessAlert::Paired {
        device: name.to_owned(),
    });
    let _ = to_agents()
        .with("Set-Cookie", session_cookie(&token))
        .write_to(stream);
}

/// Runs until the phone leaves, access stops or leaves the address, or its device
/// is revoked.
fn upgrade(mut stream: TcpStream, head: &RequestHead, visitor: Option<Visitor>, shared: &Shared) {
    let key = head.header("sec-websocket-key");
    let visitor = visitor.filter(|_| shared.access().accepts_origin(head.header("origin")));
    let (Some(key), Some(visitor), Ok(peer)) = (key, visitor, stream.peer_addr()) else {
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
    let socket = LiveSocket {
        device: visitor.id,
        name: visitor.name,
        ip: peer.ip(),
    };
    let opened_on = shared.addresses_left();
    shared.open_socket(socket.clone());
    PhoneSocket::new(
        ws,
        shared.services.registry.clone(),
        shared.services.launcher.clone(),
    )
    .run(|| {
        shared.is_stopped()
            || shared.addresses_left() != opened_on
            || !shared.is_paired(&socket.device)
    });
    shared.close_socket(&socket);
}

fn bind(ip: Ipv4Addr, devices: &PairedDevices) -> io::Result<Bound> {
    let listener = bind_remembered_port(ip, devices)?;
    listener.set_nonblocking(true)?;
    Ok(Bound { ip, listener })
}

/// The port the phone's bookmark points at, when still free; else a new one,
/// remembered.
fn bind_remembered_port(ip: Ipv4Addr, devices: &PairedDevices) -> io::Result<TcpListener> {
    let remembered = devices.read(|book| book.port);
    let listener = match remembered.and_then(|port| TcpListener::bind((ip, port)).ok()) {
        Some(listener) => listener,
        None => TcpListener::bind((ip, 0))?,
    };
    let port = listener.local_addr()?.port();
    if remembered != Some(port) {
        devices.edit(|book| book.port = Some(port));
    }
    Ok(listener)
}
