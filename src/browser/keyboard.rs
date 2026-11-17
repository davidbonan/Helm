//! Which `Cmd` chords a focused page keeps rather than Helm (browser.md §6).

/// The key pressed with `Cmd`, as the keyboard layout types it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdKey {
    Char(char),
    Backspace,
    ForwardDelete,
    Arrow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmdChord {
    pub key: CmdKey,
    pub alt: bool,
    pub ctrl: bool,
}

/// Clipboard, select-all and undo chords, plus the app menu's `Cmd+Q/H/M`.
const PAGE_LETTERS: [char; 8] = ['c', 'v', 'x', 'a', 'z', 'q', 'h', 'm'];

pub fn page_keeps(chord: CmdChord) -> bool {
    match chord.key {
        CmdKey::Char(typed) => PAGE_LETTERS.contains(&typed.to_ascii_lowercase()),
        CmdKey::Backspace | CmdKey::ForwardDelete | CmdKey::Arrow => !(chord.alt || chord.ctrl),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(key: CmdKey) -> CmdChord {
        CmdChord {
            key,
            alt: false,
            ctrl: false,
        }
    }

    #[test]
    fn page_keeps_the_text_editing_chords() {
        for key in [
            CmdKey::Char('c'),
            CmdKey::Char('V'),
            CmdKey::Char('Z'),
            CmdKey::Backspace,
            CmdKey::ForwardDelete,
            CmdKey::Arrow,
        ] {
            assert!(page_keeps(cmd(key)), "{key:?}");
        }
    }

    #[test]
    fn helm_gets_its_own_chords_and_the_pane_arrows() {
        assert!(!page_keeps(cmd(CmdKey::Char('t'))));
        let focus_pane = CmdChord {
            alt: true,
            ..cmd(CmdKey::Arrow)
        };
        let resize_pane = CmdChord {
            ctrl: true,
            ..cmd(CmdKey::Arrow)
        };
        assert!(!page_keeps(focus_pane));
        assert!(!page_keeps(resize_pane));
    }
}
