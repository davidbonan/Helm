//! The WebSocket messages (specs/remote.md §6), one JSON object per text frame.

use std::collections::BTreeMap;

use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::agent_watch::AgentBadge;
use crate::remote::launch::LaunchTargets;
use crate::remote::registry::ExposedAgent;
use crate::terminal::keys::{key_bytes, Key, Mods};
use crate::terminal::palette::TermPalette;
use crate::terminal::screen::{HistoryPage, Run, Screen, ScreenLine};
use crate::theme::Palette;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentRow {
    pub id: u64,
    pub project: String,
    pub branch: Option<String>,
    pub tab: String,
    pub badge: &'static str,
}

impl AgentRow {
    pub fn of(exposed: &ExposedAgent) -> Self {
        Self {
            id: exposed.pane.uid.get(),
            project: exposed.pane.project.clone(),
            branch: exposed.pane.branch.clone(),
            tab: exposed.pane.tab.clone(),
            badge: match exposed.reading.badge {
                AgentBadge::Working => "working",
                AgentBadge::Done => "done",
                AgentBadge::Idle | AgentBadge::None => "idle",
            },
        }
    }
}

/// A workspace entry as the phone picks it: no path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EntryChoice {
    pub id: u64,
    pub project: String,
    pub branch: Option<String>,
    pub worktree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentChoice {
    pub id: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Targets {
    pub entries: Vec<EntryChoice>,
    pub agents: Vec<AgentChoice>,
}

impl Targets {
    pub fn of(targets: &LaunchTargets) -> Self {
        Self {
            entries: targets
                .entries
                .iter()
                .map(|entry| EntryChoice {
                    id: entry.id,
                    project: entry.project.clone(),
                    branch: entry.branch.clone(),
                    worktree: entry.worktree,
                })
                .collect(),
            agents: targets
                .agents
                .iter()
                .enumerate()
                .map(|(id, agent)| AgentChoice {
                    id,
                    name: agent.name.clone(),
                })
                .collect(),
        }
    }
}

/// helm's chrome colors under the page's token names (app.css `:root`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PageTheme {
    pub dark: bool,
    pub tokens: BTreeMap<&'static str, String>,
}

impl PageTheme {
    pub fn of(palette: &Palette) -> Self {
        let tokens = [
            ("accent", palette.accent),
            ("sidebar", palette.bg_sidebar),
            ("canvas", palette.bg_canvas),
            ("surface", palette.bg_surface),
            ("surface-hover", palette.bg_surface_hover),
            ("border", palette.border_subtle),
            ("border-input", palette.border_input),
            ("text", palette.text_primary),
            ("text-secondary", palette.text_secondary),
            ("muted", palette.text_muted),
            ("added", palette.git_added),
        ];
        Self {
            dark: palette.dark,
            tokens: tokens
                .into_iter()
                .map(|(name, color)| (name, hex(color)))
                .collect(),
        }
    }

    /// Declarations for the page's `<html style>`: its first paint already wears helm's theme.
    pub fn css(&self) -> String {
        self.tokens
            .iter()
            .map(|(name, color)| format!("--{name}:{color};"))
            .collect()
    }
}

fn hex(color: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r(), color.g(), color.b())
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
    Theme(PageTheme),
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
    Targets(Targets),
    Launched {
        id: u64,
    },
    LaunchFailed {
        message: String,
    },
    /// The Mac is still there: an idle agent list sends nothing else.
    Ping,
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
    Launch {
        entry: u64,
        agent: usize,
        rows: u16,
        cols: u16,
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
    Up,
    Down,
    Tab,
    Backtab,
    Backspace,
    CtrlC,
    Enter,
}

impl QuickKey {
    /// Encoded by the Mac keyboard's own table (`terminal::keys`).
    pub fn bytes(self) -> Vec<u8> {
        let chord = match self {
            Self::Escape => (Key::Escape, Mods::NONE),
            Self::Up => (Key::ArrowUp, Mods::NONE),
            Self::Down => (Key::ArrowDown, Mods::NONE),
            Self::Tab => (Key::Tab, Mods::NONE),
            Self::Backtab => (Key::Tab, Mods::SHIFT),
            Self::Backspace => (Key::Backspace, Mods::NONE),
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
    }

    #[test]
    fn phone_messages_parse_from_their_json() {
        let key: FromPhone = serde_json::from_str(r#"{"type":"key","id":7,"key":"down"}"#).unwrap();
        let send: FromPhone =
            serde_json::from_str(r#"{"type":"send","id":7,"text":"go"}"#).unwrap();
        let watch: FromPhone =
            serde_json::from_str(r#"{"type":"watch","id":7,"rows":40,"cols":52}"#).unwrap();

        assert_eq!(
            key,
            FromPhone::Key {
                id: 7,
                key: QuickKey::Down
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
