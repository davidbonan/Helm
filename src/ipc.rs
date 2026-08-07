//! Local control socket (specs/cli.md §9): the CLI asks the **running** app about
//! the Run strips and drives them.
//!
//! `helm://` is one-way — LaunchServices delivers a URL and nothing comes back —
//! so a question like "is a server up on this worktree?" needs its own channel. A
//! Unix socket next to the prefs gives request/response, and inherits the support
//! dir's scoping for free: the dev build (`helm-dev`) never answers for the
//! installed app.
//!
//! One JSON line in, one JSON line out, connection closed. The socket thread owns
//! no state: it **parks** each request for the UI thread (the Run panes live in
//! `HelmApp`) and blocks on the reply, exactly like the `helm://` handler parks a
//! target.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// How long the socket thread waits for the UI thread. An awake app answers in
/// milliseconds; past this it is asleep — **a hidden or minimized app gets no
/// draw callbacks from macOS, so `update` never runs** and the request can only
/// be dropped (measured: hidden ⇒ no answer, unhidden ⇒ 25 ms).
const REPLY_TIMEOUT: Duration = Duration::from_secs(5);

/// The client waits longer than the app takes to give up, so a timeout is
/// reported as the app closing the connection — one story, not two.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(8);

/// What the CLI asks of the running app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Request {
    /// Every worktree of the workspace.
    List,
    /// One worktree (canonical working-tree path).
    Status { path: PathBuf },
    /// Spawn the resolved run command. A **running** process is left alone —
    /// "make sure it is up", not "restart it" (that is `Relaunch`).
    Start { path: PathBuf },
    /// Drop the pane, killing the process tree.
    Stop { path: PathBuf },
    /// Stop then Start, unconditionally.
    Relaunch { path: PathBuf },
    /// Tail of what the process printed: the strip's viewer, as text.
    Logs { path: PathBuf, lines: usize },
}

impl Request {
    /// The worktree the request targets, if any.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Request::List => None,
            Request::Status { path }
            | Request::Start { path }
            | Request::Stop { path }
            | Request::Relaunch { path }
            | Request::Logs { path, .. } => Some(path),
        }
    }
}

/// Live state of a worktree's Run process — the strip's status dot (git.md §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// No process: never started, or stopped.
    Stopped,
    Running,
    /// The command returned on its own.
    Exited,
    /// The PTY failed to spawn; `RunEntry::error` carries why.
    Failed,
}

impl RunState {
    pub fn label(self) -> &'static str {
        match self {
            RunState::Stopped => "stopped",
            RunState::Running => "running",
            RunState::Exited => "exited",
            RunState::Failed => "failed",
        }
    }
}

/// One worktree's Run strip, flattened for a caller outside the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEntry {
    pub worktree: PathBuf,
    /// Group root's name: a root and its worktrees share it (specs/agents.md §5).
    pub project: String,
    pub branch: Option<String>,
    pub state: RunState,
    /// Resolved `$PORT` for this worktree, or `None` when the command ignores it.
    pub port: Option<u16>,
    /// Stored (or auto-detected) command template, `$PORT` unsubstituted. Empty
    /// when nothing is configured and no manifest matched.
    pub command: String,
    /// What actually spawns: `command` with `$PORT` resolved.
    pub launch_command: String,
    /// Spawn failure message, on `Failed`.
    pub error: Option<String>,
    /// Code the command returned, on `Exited` — the difference between a server
    /// that was asked to stop and one that died.
    pub exit_code: Option<u32>,
}

/// Struct variants, not newtypes: an internally-tagged enum cannot serialize a
/// variant that is a bare sequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Runs {
        runs: Vec<RunEntry>,
    },
    /// The worktree's entry plus the captured output, newest last. Empty when no
    /// process ever ran there — a stopped strip keeps no buffer.
    Logs {
        entry: RunEntry,
        lines: Vec<String>,
    },
    Error {
        message: String,
    },
}

/// Why a request could not be answered. Both first variants mean "no answer to be
/// had": the caller should do the job itself rather than retry.
#[derive(Debug)]
pub enum ClientError {
    /// No socket, or nothing listening on it: helm is not running.
    NotRunning,
    /// Running but silent: its window is hidden or minimized (macOS then stops
    /// the draws that drive the frame the answer is written in), or it is wedged.
    NotAnswering,
    /// The exchange itself broke.
    Failed(String),
}

/// `<support dir>/helm.sock`, beside the prefs and the instance lock.
pub fn socket_path() -> Option<PathBuf> {
    let prefs = crate::persistence::prefs_path()?;
    Some(prefs.parent()?.join("helm.sock"))
}

/// Binds the socket and serves it on a background thread. Failures are reported
/// and swallowed: no app launch is ever blocked on the control socket.
pub fn serve() {
    let Some(path) = socket_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // A socket file outlives the process that bound it and would refuse the bind.
    // The instance lock (cli::acquire_instance_lock) has already proved no other
    // helm is live, so any file here is a leftover.
    let _ = std::fs::remove_file(&path);
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(err) => {
            eprintln!("helm: cannot open the control socket: {err}");
            return;
        }
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    std::thread::spawn(move || serve_loop(listener, park));
}

static PENDING: Mutex<Vec<(Request, Sender<Response>)>> = Mutex::new(Vec::new());
static REPAINTER: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Lets a request arriving while the app is idle wake the event loop.
pub fn arm(ctx: &egui::Context) {
    if let Ok(mut repainter) = REPAINTER.lock() {
        *repainter = Some(ctx.clone());
    }
}

/// Requests parked for the UI thread, taken at the top of a frame.
pub fn take_pending() -> Vec<(Request, Sender<Response>)> {
    PENDING
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default()
}

/// Hands the request to the UI thread and waits for its answer. `None` ⇒ no frame
/// ran in time: the connection is closed unanswered rather than dressed up as a
/// refusal, which is a different thing entirely.
fn park(request: Request) -> Option<Response> {
    let (tx, rx) = mpsc::channel();
    PENDING.lock().ok()?.push((request, tx));
    if let Ok(repainter) = REPAINTER.lock() {
        if let Some(ctx) = repainter.as_ref() {
            ctx.request_repaint();
        }
    }
    rx.recv_timeout(REPLY_TIMEOUT).ok()
}

/// Serves connections one at a time: a request is a line of JSON and an answer
/// costs a frame, so a queue behind a slow one is bounded by `REPLY_TIMEOUT`.
fn serve_loop(listener: UnixListener, dispatch: impl Fn(Request) -> Option<Response>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let _ = answer(stream, &dispatch);
    }
}

fn answer(
    mut stream: UnixStream,
    dispatch: &impl Fn(Request) -> Option<Response>,
) -> std::io::Result<()> {
    let _ = stream.set_read_timeout(Some(REPLY_TIMEOUT));
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(request) => dispatch(request),
        Err(err) => Some(Response::Error {
            message: format!("malformed request: {err}"),
        }),
    };
    // Nothing to say ⇒ close: the client reads EOF and reports a silent app.
    let Some(response) = response else {
        return Ok(());
    };
    let body = serde_json::to_string(&response)
        .unwrap_or_else(|_| r#"{"result":"error","message":"unserializable response"}"#.to_owned());
    writeln!(stream, "{body}")?;
    stream.flush()
}

/// Sends `request` to the running app. `Err` covers the exchange itself; the
/// app's own refusal comes back as `Ok(Response::Error)`.
pub fn request(request: &Request) -> Result<Response, ClientError> {
    let path = socket_path().ok_or(ClientError::NotRunning)?;
    request_at(&path, request)
}

/// [`request`] against an explicit socket path (tests).
pub fn request_at(path: &Path, request: &Request) -> Result<Response, ClientError> {
    let mut stream = UnixStream::connect(path).map_err(|err| match err.kind() {
        // No file, or a leftover socket nobody listens on.
        std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
            ClientError::NotRunning
        }
        _ => ClientError::Failed(err.to_string()),
    })?;
    let _ = stream.set_read_timeout(Some(CLIENT_TIMEOUT));
    let body =
        serde_json::to_string(request).map_err(|err| ClientError::Failed(err.to_string()))?;
    writeln!(stream, "{body}").map_err(|err| ClientError::Failed(err.to_string()))?;
    let mut line = String::new();
    let read = BufReader::new(&stream)
        .read_line(&mut line)
        .map_err(|err| match err.kind() {
            // The read timeout fires as EAGAIN here: a raw errno would say nothing.
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => {
                ClientError::NotAnswering
            }
            _ => ClientError::Failed(err.to_string()),
        })?;
    // EOF: the app gave up on its own side (no frame ran).
    if read == 0 {
        return Err(ClientError::NotAnswering);
    }
    serde_json::from_str::<Response>(line.trim())
        .map_err(|err| ClientError::Failed(format!("malformed answer: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn entry(worktree: &str, state: RunState) -> RunEntry {
        RunEntry {
            worktree: PathBuf::from(worktree),
            project: "api".to_owned(),
            branch: Some("main".to_owned()),
            state,
            port: Some(3000),
            command: "npm run dev -- --port $PORT".to_owned(),
            launch_command: "npm run dev -- --port 3000".to_owned(),
            error: None,
            exit_code: None,
        }
    }

    #[test]
    fn requests_round_trip_through_json() {
        let request = Request::Start {
            path: PathBuf::from("/dev/api"),
        };
        let text = serde_json::to_string(&request).unwrap();
        assert_eq!(text, r#"{"op":"start","path":"/dev/api"}"#);
        assert_eq!(serde_json::from_str::<Request>(&text).unwrap(), request);
    }

    #[test]
    fn the_state_is_a_stable_lowercase_string() {
        let text = serde_json::to_string(&entry("/dev/api", RunState::Running)).unwrap();
        assert!(text.contains(r#""state":"running""#), "{text}");
    }

    #[test]
    fn a_round_trip_carries_the_entries_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("helm.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            serve_loop(listener, |request| match request {
                Request::List => Some(Response::Runs {
                    runs: vec![entry("/dev/api", RunState::Running)],
                }),
                _ => Some(Response::Error {
                    message: "unexpected".to_owned(),
                }),
            })
        });

        let answer = request_at(&path, &Request::List).unwrap();
        assert_eq!(
            answer,
            Response::Runs {
                runs: vec![entry("/dev/api", RunState::Running)]
            }
        );
    }

    #[test]
    fn a_round_trip_carries_the_captured_output_back() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("helm.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            serve_loop(listener, |request| match request {
                Request::Logs { lines, .. } => Some(Response::Logs {
                    entry: entry("/dev/api", RunState::Running),
                    lines: vec![format!("listening on :3000 ({lines} asked)")],
                }),
                _ => Some(Response::Error {
                    message: "unexpected".to_owned(),
                }),
            })
        });

        let answer = request_at(
            &path,
            &Request::Logs {
                path: PathBuf::from("/dev/api"),
                lines: 40,
            },
        )
        .unwrap();
        assert_eq!(
            answer,
            Response::Logs {
                entry: entry("/dev/api", RunState::Running),
                lines: vec!["listening on :3000 (40 asked)".to_owned()],
            }
        );
    }

    #[test]
    fn an_app_refusal_comes_back_as_an_error_answer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("helm.sock");
        let listener = UnixListener::bind(&path).unwrap();
        std::thread::spawn(move || {
            serve_loop(listener, |_| {
                Some(Response::Error {
                    message: "no run command".to_owned(),
                })
            })
        });

        let answer = request_at(&path, &Request::List).unwrap();
        assert!(
            matches!(&answer, Response::Error { message } if message == "no run command"),
            "{answer:?}"
        );
    }

    #[test]
    fn an_app_that_says_nothing_reads_as_not_answering() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("helm.sock");
        let listener = UnixListener::bind(&path).unwrap();
        // What a hidden window produces: no frame ran, so the socket thread has
        // nothing to write and closes.
        std::thread::spawn(move || serve_loop(listener, |_| None));

        let err = request_at(&path, &Request::List).unwrap_err();
        assert!(
            matches!(err, ClientError::NotAnswering),
            "silence is not a refusal: {err:?}"
        );
    }

    #[test]
    fn no_listener_reads_as_not_running() {
        let dir = tempdir().unwrap();
        let err = request_at(&dir.path().join("absent.sock"), &Request::List).unwrap_err();
        assert!(matches!(err, ClientError::NotRunning), "{err:?}");
    }
}
