//! Keys → bytes sent to the PTY, shared by the Mac keyboard (`ui::terminal_view`)
//! and the phone's quick keys (specs/remote.md §7): one table, one encoding.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Enter,
    Tab,
    Backspace,
    Escape,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    /// An ASCII letter, uppercase.
    Letter(char),
}

/// Normalized modifier signature, compared exactly by the chords.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Mods {
    pub cmd: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
}

impl Mods {
    pub const NONE: Self = Self {
        cmd: false,
        alt: false,
        ctrl: false,
        shift: false,
    };
    pub const CMD: Self = Self {
        cmd: true,
        ..Self::NONE
    };
    pub const ALT: Self = Self {
        alt: true,
        ..Self::NONE
    };
    pub const CTRL: Self = Self {
        ctrl: true,
        ..Self::NONE
    };
    pub const SHIFT: Self = Self {
        shift: true,
        ..Self::NONE
    };
}

/// Chords forwarded to the PTY, selected by exact modifier match. Adding a line
/// is enough to bind a new shortcut.
const CHORDS: &[(Mods, Key, &[u8])] = &[
    (Mods::SHIFT, Key::Tab, b"\x1b[Z"), // backtab (CSI Z)
    // Shift+Enter: CSI u **without negotiation** (kitty/Ghostty convention for
    // combos without a legacy encoding). Claude Code never pushes the kitty
    // protocol: it parses `CSI 13;2u` unconditionally and relies on the terminal
    // to emit it by default — gating on the push breaks it.
    (Mods::SHIFT, Key::Enter, b"\x1b[13;2u"),
    (Mods::ALT, Key::Enter, b"\x1b\r"), // meta+enter: Claude Code newline (/terminal-setup)
    (Mods::ALT, Key::ArrowLeft, b"\x1bb"), // previous word
    (Mods::ALT, Key::ArrowRight, b"\x1bf"), // next word
    (Mods::ALT, Key::Backspace, b"\x1b\x7f"), // delete previous word
    (Mods::CMD, Key::ArrowLeft, b"\x01"), // start of line
    (Mods::CMD, Key::ArrowRight, b"\x05"), // end of line
    (Mods::CMD, Key::Backspace, b"\x15"), // delete to start of line
];

/// Special keys without a chord; residual modifiers are ignored.
const SPECIAL: &[(Key, &[u8])] = &[
    (Key::Enter, b"\r"),
    (Key::Tab, b"\t"),
    (Key::Backspace, b"\x7f"),
    (Key::Escape, b"\x1b"),
    (Key::Delete, b"\x1b[3~"),
    (Key::ArrowUp, b"\x1b[A"),
    (Key::ArrowDown, b"\x1b[B"),
    (Key::ArrowRight, b"\x1b[C"),
    (Key::ArrowLeft, b"\x1b[D"),
];

pub fn key_bytes(key: Key, mods: Mods) -> Option<Vec<u8>> {
    if let Some((_, _, seq)) = CHORDS.iter().find(|(m, k, _)| *m == mods && *k == key) {
        return Some(seq.to_vec());
    }
    // Other Cmd combinations belong to the app (split, zoom, sidebar).
    if mods.cmd {
        return None;
    }
    if mods.ctrl {
        return ctrl_byte(key).map(|b| vec![b]);
    }
    SPECIAL
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, seq)| seq.to_vec())
}

fn ctrl_byte(key: Key) -> Option<u8> {
    match key {
        Key::Letter(c) if c.is_ascii_uppercase() => Some(c as u8 - b'A' + 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ctrl_letters_map_to_control_bytes() {
        assert_eq!(key_bytes(Key::Letter('C'), Mods::CTRL), Some(vec![0x03]));
        assert_eq!(key_bytes(Key::Letter('D'), Mods::CTRL), Some(vec![0x04]));
        assert_eq!(key_bytes(Key::Letter('Z'), Mods::CTRL), Some(vec![0x1a]));
    }

    #[test]
    fn shift_tab_sends_backtab_and_plain_tab_a_tab() {
        assert_eq!(key_bytes(Key::Tab, Mods::SHIFT), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_bytes(Key::Tab, Mods::NONE), Some(b"\t".to_vec()));
    }

    #[test]
    fn special_keys_map_to_terminal_sequences() {
        assert_eq!(key_bytes(Key::Enter, Mods::NONE), Some(b"\r".to_vec()));
        assert_eq!(
            key_bytes(Key::Backspace, Mods::NONE),
            Some(b"\x7f".to_vec())
        );
        assert_eq!(
            key_bytes(Key::ArrowUp, Mods::NONE),
            Some(b"\x1b[A".to_vec())
        );
    }
}
