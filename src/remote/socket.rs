//! One connected phone (specs/remote.md §6): a single thread alternates between
//! reading its messages (short read timeout) and pushing what changed — the agent
//! list and the watched screen, at most every [`FRAME`].

use std::io::ErrorKind;
use std::net::TcpStream;
use std::time::{Duration, Instant};

use tungstenite::{Message, WebSocket};

use crate::remote::protocol::{AgentRow, FromPhone, ToPhone};
use crate::remote::registry::Registry;
use crate::terminal::pane::PaneHandle;
use crate::terminal::screen::{history, screen};

pub const READ_TIMEOUT: Duration = Duration::from_millis(100);

const FRAME: Duration = Duration::from_millis(100);

/// Scrollback lines a single `history` request may ask for.
const MAX_HISTORY_PAGE: usize = 500;

pub struct PhoneSocket {
    ws: WebSocket<TcpStream>,
    registry: Registry,
    watched: Option<u64>,
    sent_agents: Option<Vec<AgentRow>>,
    sent_screen: Option<ToPhone>,
    last_push: Option<Instant>,
}

impl PhoneSocket {
    pub fn new(ws: WebSocket<TcpStream>, registry: Registry) -> Self {
        Self {
            ws,
            registry,
            watched: None,
            sent_agents: None,
            sent_screen: None,
            last_push: None,
        }
    }

    /// Runs until the phone leaves, the connection fails or `stopped` turns true.
    pub fn run(mut self, stopped: impl Fn() -> bool) {
        while !stopped() {
            match self.ws.read() {
                Ok(Message::Text(text)) => {
                    if let Ok(message) = serde_json::from_str::<FromPhone>(&text) {
                        if self.answer(message).is_err() {
                            return;
                        }
                    }
                }
                Ok(Message::Close(_)) => return,
                Ok(_) => {}
                Err(tungstenite::Error::Io(err))
                    if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
            if self.last_push.is_none_or(|at| at.elapsed() >= FRAME) {
                self.last_push = Some(Instant::now());
                if self.push_changes().is_err() {
                    return;
                }
            }
        }
        let _ = self.ws.close(None);
        let _ = self.ws.flush();
    }

    fn answer(&mut self, message: FromPhone) -> tungstenite::Result<()> {
        match message {
            FromPhone::Watch { id } => {
                let listed = self
                    .registry
                    .agents()
                    .iter()
                    .any(|agent| agent.pane.uid.get() == id);
                if listed {
                    self.watched = Some(id);
                    self.sent_screen = None;
                    self.last_push = None;
                }
            }
            FromPhone::Send { id, text } => self.type_into(id, |pane| {
                pane.paste(&text)?;
                pane.feed(b"\r")
            }),
            FromPhone::Key { id, key } => self.type_into(id, |pane| pane.feed(&key.bytes())),
            FromPhone::History { id, before, count } => {
                if self.watched != Some(id) {
                    return Ok(());
                }
                let Some(pane) = self.registry.pane(id) else {
                    return Ok(());
                };
                let page = history(
                    &pane.handle.grid().lock(),
                    &self.registry.palette(),
                    before,
                    count.min(MAX_HISTORY_PAGE),
                );
                return self.push(&ToPhone::history(id, &page));
            }
        }
        Ok(())
    }

    /// Input goes only to a listed pane with an agent in its foreground.
    fn type_into(&self, id: u64, write: impl FnOnce(&PaneHandle) -> anyhow::Result<()>) {
        if !self.registry.is_writable(id) {
            return;
        }
        if let Some(pane) = self.registry.pane(id) {
            let _ = write(&pane.handle);
        }
    }

    fn push_changes(&mut self) -> tungstenite::Result<()> {
        let agents: Vec<AgentRow> = self.registry.agents().iter().map(AgentRow::of).collect();
        if self.sent_agents.as_ref() != Some(&agents) {
            self.push(&ToPhone::Agents {
                agents: agents.clone(),
            })?;
            self.sent_agents = Some(agents);
        }
        let Some(id) = self.watched else {
            return Ok(());
        };
        let Some(pane) = self.registry.pane(id) else {
            self.watched = None;
            self.sent_screen = None;
            return self.push(&ToPhone::Ended { id });
        };
        let grid = screen(&pane.handle.grid().lock(), &self.registry.palette());
        let frame = ToPhone::screen(id, &grid, self.registry.is_writable(id));
        if self.sent_screen.as_ref() != Some(&frame) {
            self.push(&frame)?;
            self.sent_screen = Some(frame);
        }
        Ok(())
    }

    fn push(&mut self, message: &ToPhone) -> tungstenite::Result<()> {
        let json = serde_json::to_string(message).expect("wire messages serialize");
        self.ws.send(Message::text(json))
    }
}
