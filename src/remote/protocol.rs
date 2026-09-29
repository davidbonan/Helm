//! The WebSocket messages (specs/remote.md §6), one JSON object per text frame.

use serde::{Deserialize, Serialize};

use crate::agent_watch::{display_name, AgentBadge};
use crate::remote::registry::ExposedAgent;
use crate::terminal::keys::{key_bytes, Key, Mods};
use crate::terminal::palette::TermPalette;
use crate::terminal::screen::{HistoryPage, Run, Screen, ScreenLine};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentRow {
    pub id: u64,
    pub project: String,
    pub branch: Option<String>,
    pub tab: String,
    pub agent: String,
    pub badge: &'static str,
}

impl AgentRow {
    pub fn of(exposed: &ExposedAgent) -> Self {
        Self {
            id: exposed.pane.uid.get(),
            project: exposed.pane.project.clone(),
            branch: exposed.pane.branch.clone(),
            tab: exposed.pane.tab.clone(),
            agent: display_name(exposed.reading.agent.unwrap_or("agent")),
            badge: match exposed.reading.badge {
                AgentBadge::Working => "working",
                AgentBadge::Done => "done",
                AgentBadge::Idle | AgentBadge::None => "idle",
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WireRun {
    pub t: String,
    pub fg: String,
    pub bg: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
}

impl WireRun {
    fn of(run: &Run) -> Self {
        Self {
            t: run.text.clone(),
            fg: run.style.fg.to_hex(),
            bg: run.style.bg.to_hex(),
            bold: run.style.bold,
            italic: run.style.italic,
            underline: run.style.underline,
        }
    }
}

pub fn wire_lines(lines: &[ScreenLine]) -> Vec<Vec<WireRun>> {
    lines
        .iter()
        .map(|line| line.iter().map(WireRun::of).collect())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToPhone {
    Agents {
        agents: Vec<AgentRow>,
    },
    Screen {
        id: u64,
        cols: usize,
        rows: usize,
        /// The palette's own ink and background: what a blank cell shows.
        fg: String,
        bg: String,
        lines: Vec<Vec<WireRun>>,
        cursor: Option<(usize, usize)>,
        writable: bool,
        app_scrolls: bool,
    },
    History {
        id: u64,
        first: i32,
        lines: Vec<Vec<WireRun>>,
    },
    Ended {
        id: u64,
    },
}

impl ToPhone {
    pub fn screen(id: u64, screen: &Screen, palette: &TermPalette, writable: bool) -> Self {
        Self::Screen {
            id,
            cols: screen.cols,
            rows: screen.lines.len(),
            fg: palette.foreground.to_hex(),
            bg: palette.background.to_hex(),
            lines: wire_lines(&screen.lines),
            cursor: screen.cursor,
            writable,
            app_scrolls: screen.app_scrolls,
        }
    }

    pub fn history(id: u64, page: &HistoryPage) -> Self {
        Self::History {
            id,
            first: page.first,
            lines: wire_lines(&page.lines),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FromPhone {
    Watch {
        id: u64,
        rows: u16,
        cols: u16,
    },
    Resize {
        id: u64,
        rows: u16,
        cols: u16,
    },
    Unwatch,
    Send {
        id: u64,
        text: String,
    },
    Key {
        id: u64,
        key: QuickKey,
    },
    History {
        id: u64,
        before: i32,
        count: usize,
    },
    /// `lines > 0` = upward, at the cell under the finger — the Mac wheel's encoding.
    Scroll {
        id: u64,
        lines: i32,
        line: usize,
        col: usize,
    },
}

/// The phone's quick-key row (specs/remote.md §7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuickKey {
    Escape,
    #[serde(rename = "1")]
    One,
    #[serde(rename = "2")]
    Two,
    #[serde(rename = "3")]
    Three,
    Up,
    Down,
    Tab,
    Backtab,
    CtrlC,
    Enter,
}

impl QuickKey {
    /// Encoded by the Mac keyboard's own table (`terminal::keys`).
    pub fn bytes(self) -> Vec<u8> {
        let chord = match self {
            Self::One => return b"1".to_vec(),
            Self::Two => return b"2".to_vec(),
            Self::Three => return b"3".to_vec(),
            Self::Escape => (Key::Escape, Mods::NONE),
            Self::Up => (Key::ArrowUp, Mods::NONE),
            Self::Down => (Key::ArrowDown, Mods::NONE),
            Self::Tab => (Key::Tab, Mods::NONE),
            Self::Backtab => (Key::Tab, Mods::SHIFT),
            Self::CtrlC => (Key::Letter('C'), Mods::CTRL),
            Self::Enter => (Key::Enter, Mods::NONE),
        };
        key_bytes(chord.0, chord.1).expect("every quick key has an encoding")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_keys_use_the_terminal_encoding() {
        assert_eq!(QuickKey::Backtab.bytes(), b"\x1b[Z");
        assert_eq!(QuickKey::CtrlC.bytes(), [0x03]);
        assert_eq!(QuickKey::Escape.bytes(), b"\x1b");
        assert_eq!(QuickKey::Two.bytes(), b"2");
    }

    #[test]
    fn phone_messages_parse_from_their_json() {
        let key: FromPhone = serde_json::from_str(r#"{"type":"key","id":7,"key":"1"}"#).unwrap();
        let send: FromPhone =
            serde_json::from_str(r#"{"type":"send","id":7,"text":"go"}"#).unwrap();
        let watch: FromPhone =
            serde_json::from_str(r#"{"type":"watch","id":7,"rows":40,"cols":52}"#).unwrap();

        assert_eq!(
            key,
            FromPhone::Key {
                id: 7,
                key: QuickKey::One
            }
        );
        assert_eq!(
            send,
            FromPhone::Send {
                id: 7,
                text: "go".to_owned()
            }
        );
        assert_eq!(
            watch,
            FromPhone::Watch {
                id: 7,
                rows: 40,
                cols: 52
            }
        );
    }
}
