//! Domain keymap (keybindings.md §6): the curated rebindable actions, shortcut
//! parse/format/display, and resolution of user overrides over the spec defaults.

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Global,
    Terminal,
    Git,
}

impl Group {
    pub fn label(self) -> &'static str {
        match self {
            Self::Global => "Global",
            Self::Terminal => "Terminal",
            Self::Git => "Git",
        }
    }
}

/// The curated rebindable set (keybindings.md §6) — everything else is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Action {
    OpenFolder,
    NewTab,
    TogglePreferences,
    ToggleWorkspaceSidebar,
    ToggleGitSidebar,
    ToggleGraph,
    NextRepo,
    PrevRepo,
    SplitRight,
    SplitDown,
    ClosePane,
    FocusLeft,
    FocusRight,
    FocusUp,
    FocusDown,
    ResizeLeft,
    ResizeRight,
    ResizeUp,
    ResizeDown,
    ZoomIn,
    ZoomOut,
    ZoomReset,
    ClearTerminal,
    Commit,
    Run,
}

impl Action {
    pub const ALL: [Self; 25] = [
        Self::OpenFolder,
        Self::NewTab,
        Self::TogglePreferences,
        Self::ToggleWorkspaceSidebar,
        Self::ToggleGitSidebar,
        Self::ToggleGraph,
        Self::NextRepo,
        Self::PrevRepo,
        Self::SplitRight,
        Self::SplitDown,
        Self::ClosePane,
        Self::FocusLeft,
        Self::FocusRight,
        Self::FocusUp,
        Self::FocusDown,
        Self::ResizeLeft,
        Self::ResizeRight,
        Self::ResizeUp,
        Self::ResizeDown,
        Self::ZoomIn,
        Self::ZoomOut,
        Self::ZoomReset,
        Self::ClearTerminal,
        Self::Commit,
        Self::Run,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::OpenFolder => "open-folder",
            Self::NewTab => "new-tab",
            Self::TogglePreferences => "toggle-preferences",
            Self::ToggleWorkspaceSidebar => "toggle-workspace-sidebar",
            Self::ToggleGitSidebar => "toggle-git-sidebar",
            Self::ToggleGraph => "toggle-graph",
            Self::NextRepo => "next-repo",
            Self::PrevRepo => "prev-repo",
            Self::SplitRight => "split-right",
            Self::SplitDown => "split-down",
            Self::ClosePane => "close-pane",
            Self::FocusLeft => "focus-left",
            Self::FocusRight => "focus-right",
            Self::FocusUp => "focus-up",
            Self::FocusDown => "focus-down",
            Self::ResizeLeft => "resize-left",
            Self::ResizeRight => "resize-right",
            Self::ResizeUp => "resize-up",
            Self::ResizeDown => "resize-down",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::ZoomReset => "zoom-reset",
            Self::ClearTerminal => "clear-terminal",
            Self::Commit => "commit",
            Self::Run => "run",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenFolder => "Open Folder",
            Self::NewTab => "New Tab",
            Self::TogglePreferences => "Toggle Preferences",
            Self::ToggleWorkspaceSidebar => "Toggle workspace sidebar",
            Self::ToggleGitSidebar => "Toggle git sidebar",
            // "/" rather than the spec's "⇄": the bundled fonts have no glyph
            // for U+21C4 (renders as tofu in the Keyboard section).
            Self::ToggleGraph => "Toggle Terminal / Git",
            Self::NextRepo => "Next repo",
            Self::PrevRepo => "Previous repo",
            Self::SplitRight => "Split right",
            Self::SplitDown => "Split down",
            Self::ClosePane => "Close pane",
            Self::FocusLeft => "Focus left",
            Self::FocusRight => "Focus right",
            Self::FocusUp => "Focus up",
            Self::FocusDown => "Focus down",
            Self::ResizeLeft => "Resize left",
            Self::ResizeRight => "Resize right",
            Self::ResizeUp => "Resize up",
            Self::ResizeDown => "Resize down",
            Self::ZoomIn => "Zoom in",
            Self::ZoomOut => "Zoom out",
            Self::ZoomReset => "Reset zoom",
            Self::ClearTerminal => "Clear terminal",
            Self::Commit => "Commit",
            Self::Run => "Run / Relaunch",
        }
    }

    pub fn group(self) -> Group {
        match self {
            Self::OpenFolder
            | Self::NewTab
            | Self::TogglePreferences
            | Self::ToggleWorkspaceSidebar
            | Self::ToggleGitSidebar
            | Self::ToggleGraph
            | Self::NextRepo
            | Self::PrevRepo
            | Self::Run => Group::Global,
            Self::SplitRight
            | Self::SplitDown
            | Self::ClosePane
            | Self::FocusLeft
            | Self::FocusRight
            | Self::FocusUp
            | Self::FocusDown
            | Self::ResizeLeft
            | Self::ResizeRight
            | Self::ResizeUp
            | Self::ResizeDown
            | Self::ZoomIn
            | Self::ZoomOut
            | Self::ZoomReset
            | Self::ClearTerminal => Group::Terminal,
            Self::Commit => Group::Git,
        }
    }

    /// Defaults of keybindings.md §6 — the §1–§3 tables.
    pub fn default_shortcut(self) -> Shortcut {
        use egui::Key;
        match self {
            Self::OpenFolder => Shortcut::cmd(Key::O),
            Self::NewTab => Shortcut::cmd(Key::T),
            Self::TogglePreferences => Shortcut::cmd(Key::Comma),
            Self::ToggleWorkspaceSidebar => Shortcut::cmd(Key::B),
            Self::ToggleGitSidebar => Shortcut::cmd(Key::G),
            Self::ToggleGraph => Shortcut::cmd_shift(Key::G),
            Self::NextRepo => Shortcut::ctrl(Key::Tab),
            Self::PrevRepo => Shortcut::ctrl_shift(Key::Tab),
            Self::SplitRight => Shortcut::cmd(Key::D),
            Self::SplitDown => Shortcut::cmd_shift(Key::D),
            Self::ClosePane => Shortcut::cmd(Key::W),
            Self::FocusLeft => Shortcut::cmd_alt(Key::ArrowLeft),
            Self::FocusRight => Shortcut::cmd_alt(Key::ArrowRight),
            Self::FocusUp => Shortcut::cmd_alt(Key::ArrowUp),
            Self::FocusDown => Shortcut::cmd_alt(Key::ArrowDown),
            Self::ResizeLeft => Shortcut::cmd_ctrl(Key::ArrowLeft),
            Self::ResizeRight => Shortcut::cmd_ctrl(Key::ArrowRight),
            Self::ResizeUp => Shortcut::cmd_ctrl(Key::ArrowUp),
            Self::ResizeDown => Shortcut::cmd_ctrl(Key::ArrowDown),
            Self::ZoomIn => Shortcut::cmd(Key::Equals),
            Self::ZoomOut => Shortcut::cmd(Key::Minus),
            Self::ZoomReset => Shortcut::cmd(Key::Num0),
            Self::ClearTerminal => Shortcut::cmd(Key::K),
            Self::Commit => Shortcut::cmd(Key::Enter),
            Self::Run => Shortcut::cmd(Key::R),
        }
    }
}

/// One non-modifier key plus modifiers (keybindings.md §6 binding rules).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub cmd: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub key: egui::Key,
}

impl Shortcut {
    pub const fn cmd(key: egui::Key) -> Self {
        Self {
            cmd: true,
            ctrl: false,
            alt: false,
            shift: false,
            key,
        }
    }

    pub const fn cmd_shift(key: egui::Key) -> Self {
        Self {
            shift: true,
            ..Self::cmd(key)
        }
    }

    pub const fn cmd_alt(key: egui::Key) -> Self {
        Self {
            alt: true,
            ..Self::cmd(key)
        }
    }

    pub const fn cmd_ctrl(key: egui::Key) -> Self {
        Self {
            ctrl: true,
            ..Self::cmd(key)
        }
    }

    pub const fn ctrl(key: egui::Key) -> Self {
        Self {
            cmd: false,
            ctrl: true,
            alt: false,
            shift: false,
            key,
        }
    }

    pub const fn ctrl_shift(key: egui::Key) -> Self {
        Self {
            shift: true,
            ..Self::ctrl(key)
        }
    }

    /// Parses a canonical combo (`"cmd+shift+d"`): modifier tokens then one key
    /// token, case-insensitive; symbol forms (`"cmd+="`) accepted via egui.
    pub fn parse(combo: &str) -> Option<Self> {
        let tokens: Vec<&str> = combo.split('+').collect();
        let (key_token, modifier_tokens) = tokens.split_last()?;
        let mut shortcut = Self {
            cmd: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: key_from_token(key_token)?,
        };
        for token in modifier_tokens {
            if token.eq_ignore_ascii_case("cmd") {
                shortcut.cmd = true;
            } else if token.eq_ignore_ascii_case("ctrl") {
                shortcut.ctrl = true;
            } else if token.eq_ignore_ascii_case("alt") {
                shortcut.alt = true;
            } else if token.eq_ignore_ascii_case("shift") {
                shortcut.shift = true;
            } else {
                return None;
            }
        }
        Some(shortcut)
    }

    /// Canonical persisted form (preferences.md §5), e.g. `"cmd+shift+d"`.
    pub fn canonical(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.cmd {
            parts.push("cmd");
        }
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.shift {
            parts.push("shift");
        }
        let key = self.key.name().to_ascii_lowercase();
        parts.push(&key);
        parts.join("+")
    }

    /// Mac display in the existing badge convention (`⌃⌥⇧⌘` order, e.g. `⇧⌘D`).
    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.ctrl {
            out.push('⌃');
        }
        if self.alt {
            out.push('⌥');
        }
        if self.shift {
            out.push('⇧');
        }
        if self.cmd {
            out.push('⌘');
        }
        out.push_str(key_display(self.key));
        out
    }

    /// Exact-modifier match against an egui key event.
    pub fn matches(&self, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        key == self.key
            && modifiers.command == self.cmd
            && modifiers.ctrl == self.ctrl
            && modifiers.alt == self.alt
            && modifiers.shift == self.shift
    }

    /// Refused at capture and ignored at resolution (keybindings.md §6): the
    /// positional ranges `Cmd+1..9` / `Cmd+Ctrl+1..9`, `Esc`, and combos without
    /// `Cmd`/`Ctrl`/`Alt` (`Shift` alone would swallow typing).
    pub fn is_reserved(&self) -> bool {
        use egui::Key;
        if !(self.cmd || self.ctrl || self.alt) {
            return true;
        }
        if self.key == Key::Escape {
            return true;
        }
        let digit = matches!(
            self.key,
            Key::Num1
                | Key::Num2
                | Key::Num3
                | Key::Num4
                | Key::Num5
                | Key::Num6
                | Key::Num7
                | Key::Num8
                | Key::Num9
        );
        digit && self.cmd && !self.alt && !self.shift
    }
}

fn key_from_token(token: &str) -> Option<egui::Key> {
    egui::Key::from_name(token).or_else(|| {
        egui::Key::ALL
            .iter()
            .copied()
            .find(|key| key.name().eq_ignore_ascii_case(token))
    })
}

fn key_display(key: egui::Key) -> &'static str {
    use egui::Key;
    match key {
        Key::ArrowLeft => "←",
        Key::ArrowRight => "→",
        Key::ArrowUp => "↑",
        Key::ArrowDown => "↓",
        Key::Enter => "↩",
        Key::Escape => "⎋",
        Key::Backspace => "⌫",
        Key::Delete => "⌦",
        Key::Tab => "⇥",
        _ => key.symbol_or_name(),
    }
}

/// Resolved bindings: the spec defaults plus the user's deviations
/// (`None` = unbound).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Keymap {
    overrides: BTreeMap<Action, Option<Shortcut>>,
}

impl Keymap {
    /// Resolves the persisted `keybindings` table (preferences.md §5): `""` =
    /// unbound; an unknown action id or an unparsable/reserved combo is ignored
    /// (the default applies) without rewriting the source.
    pub fn resolve(entries: &BTreeMap<String, String>) -> Self {
        let mut keymap = Self::default();
        for (id, combo) in entries {
            let Some(action) = Action::from_id(id) else {
                continue;
            };
            if combo.is_empty() {
                keymap.set(action, None);
            } else if let Some(shortcut) = Shortcut::parse(combo) {
                if !shortcut.is_reserved() {
                    keymap.set(action, Some(shortcut));
                }
            }
        }
        keymap
    }

    /// Current binding of `action` (override or spec default). `None` = unbound:
    /// never matched, no badge (keybindings.md §5–§6).
    pub fn shortcut_for(&self, action: Action) -> Option<Shortcut> {
        match self.overrides.get(&action) {
            Some(overridden) => *overridden,
            None => Some(action.default_shortcut()),
        }
    }

    pub fn matches(&self, action: Action, key: egui::Key, modifiers: egui::Modifiers) -> bool {
        self.shortcut_for(action)
            .is_some_and(|shortcut| shortcut.matches(key, modifiers))
    }

    /// The action currently holding `shortcut`, for the capture-time conflict
    /// error — no silent stealing (keybindings.md §6).
    pub fn holder_of(&self, shortcut: Shortcut) -> Option<Action> {
        Action::ALL
            .into_iter()
            .find(|action| self.shortcut_for(*action) == Some(shortcut))
    }

    /// Binds `action` (`None` = unbind). Binding the spec default stores no
    /// deviation.
    pub fn set(&mut self, action: Action, shortcut: Option<Shortcut>) {
        if shortcut == Some(action.default_shortcut()) {
            self.overrides.remove(&action);
        } else {
            self.overrides.insert(action, shortcut);
        }
    }

    pub fn reset(&mut self, action: Action) {
        self.overrides.remove(&action);
    }

    pub fn restore_defaults(&mut self) {
        self.overrides.clear();
    }

    pub fn deviates(&self, action: Action) -> bool {
        self.overrides.contains_key(&action)
    }

    /// Deviations from the defaults, in stable order — what persistence writes
    /// (`None` = unbound, serialized `""`).
    pub fn deviations(&self) -> impl Iterator<Item = (Action, Option<Shortcut>)> + '_ {
        self.overrides
            .iter()
            .map(|(action, shortcut)| (*action, *shortcut))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Key, Modifiers};

    fn mods(cmd: bool, ctrl: bool, alt: bool, shift: bool) -> Modifiers {
        Modifiers {
            command: cmd,
            mac_cmd: cmd,
            ctrl,
            alt,
            shift,
        }
    }

    fn entries(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(id, combo)| (id.to_string(), combo.to_string()))
            .collect()
    }

    #[test]
    fn parse_canonical_round_trip_for_all_defaults() {
        for action in Action::ALL {
            let default = action.default_shortcut();
            let canonical = default.canonical();
            assert_eq!(
                Shortcut::parse(&canonical),
                Some(default),
                "round-trip failed for {} ({canonical})",
                action.id()
            );
        }
    }

    #[test]
    fn parse_accepts_case_and_symbol_forms() {
        let cmd_shift_d = Shortcut::cmd_shift(Key::D);
        assert_eq!(Shortcut::parse("cmd+shift+d"), Some(cmd_shift_d));
        assert_eq!(Shortcut::parse("Cmd+Shift+D"), Some(cmd_shift_d));
        assert_eq!(Shortcut::parse("cmd+="), Some(Shortcut::cmd(Key::Equals)));
        assert_eq!(Shortcut::parse("cmd+,"), Some(Shortcut::cmd(Key::Comma)));
        assert_eq!(
            Shortcut::parse("cmd+ctrl+left"),
            Some(Shortcut::cmd_ctrl(Key::ArrowLeft))
        );
    }

    #[test]
    fn parse_rejects_garbage() {
        for combo in ["", "cmd", "cmd+", "cmd+banana", "banana+d", "d+cmd"] {
            assert_eq!(Shortcut::parse(combo), None, "{combo:?} should not parse");
        }
    }

    #[test]
    fn display_uses_mac_glyph_order() {
        assert_eq!(Shortcut::cmd_shift(Key::D).display(), "⇧⌘D");
        assert_eq!(Shortcut::cmd_ctrl(Key::ArrowLeft).display(), "⌃⌘←");
        assert_eq!(Shortcut::cmd_alt(Key::ArrowUp).display(), "⌥⌘↑");
        assert_eq!(Shortcut::cmd(Key::Comma).display(), "⌘,");
        assert_eq!(Shortcut::cmd(Key::Num0).display(), "⌘0");
        assert_eq!(Shortcut::cmd(Key::Enter).display(), "⌘↩");
    }

    #[test]
    fn defaults_are_complete_and_conflict_free() {
        for (i, a) in Action::ALL.into_iter().enumerate() {
            let shortcut = a.default_shortcut();
            assert!(!shortcut.is_reserved(), "{} default is reserved", a.id());
            for b in &Action::ALL[i + 1..] {
                assert_ne!(
                    shortcut,
                    b.default_shortcut(),
                    "{} and {} share a default",
                    a.id(),
                    b.id()
                );
            }
        }
    }

    #[test]
    fn ids_are_unique_and_resolve_back() {
        for action in Action::ALL {
            assert_eq!(Action::from_id(action.id()), Some(action));
        }
        assert_eq!(Action::from_id("does-not-exist"), None);
    }

    #[test]
    fn resolve_applies_override_and_kills_default() {
        let keymap = Keymap::resolve(&entries(&[("split-right", "cmd+shift+x")]));
        assert_eq!(
            keymap.shortcut_for(Action::SplitRight),
            Some(Shortcut::cmd_shift(Key::X))
        );
        assert!(keymap.matches(Action::SplitRight, Key::X, mods(true, false, false, true)));
        assert!(!keymap.matches(Action::SplitRight, Key::D, mods(true, false, false, false)));
    }

    #[test]
    fn resolve_unbinds_on_empty_string() {
        let keymap = Keymap::resolve(&entries(&[("split-right", "")]));
        assert_eq!(keymap.shortcut_for(Action::SplitRight), None);
        assert!(!keymap.matches(Action::SplitRight, Key::D, mods(true, false, false, false)));
    }

    #[test]
    fn resolve_ignores_unknown_id_and_bad_combos() {
        let keymap = Keymap::resolve(&entries(&[
            ("does-not-exist", "cmd+x"),
            ("split-right", "cmd+banana"),
            ("close-pane", "cmd+1"),
            ("clear-terminal", "k"),
        ]));
        assert_eq!(keymap, Keymap::default());
        assert!(keymap.matches(Action::SplitRight, Key::D, mods(true, false, false, false)));
        assert!(keymap.matches(Action::ClosePane, Key::W, mods(true, false, false, false)));
        assert!(keymap.matches(
            Action::ClearTerminal,
            Key::K,
            mods(true, false, false, false)
        ));
    }

    #[test]
    fn reserved_ranges_esc_and_modifierless() {
        assert!(Shortcut::cmd(Key::Num1).is_reserved());
        assert!(Shortcut::cmd(Key::Num9).is_reserved());
        assert!(Shortcut::cmd_ctrl(Key::Num5).is_reserved());
        assert!(Shortcut::cmd(Key::Escape).is_reserved());
        let shift_only = Shortcut {
            cmd: false,
            ctrl: false,
            alt: false,
            shift: true,
            key: Key::D,
        };
        assert!(shift_only.is_reserved());
        let bare = Shortcut {
            cmd: false,
            ctrl: false,
            alt: false,
            shift: false,
            key: Key::D,
        };
        assert!(bare.is_reserved());
        assert!(!Shortcut::cmd(Key::Num0).is_reserved());
        assert!(!Shortcut::cmd_shift(Key::Num1).is_reserved());
        assert!(!Shortcut::cmd(Key::K).is_reserved());
        assert!(!Shortcut::cmd_alt(Key::ArrowLeft).is_reserved());
    }

    #[test]
    fn holder_of_reflects_current_bindings() {
        let mut keymap = Keymap::default();
        assert_eq!(
            keymap.holder_of(Shortcut::cmd(Key::D)),
            Some(Action::SplitRight)
        );
        keymap.set(Action::SplitRight, None);
        assert_eq!(keymap.holder_of(Shortcut::cmd(Key::D)), None);
        keymap.set(Action::SplitRight, Some(Shortcut::cmd_shift(Key::X)));
        assert_eq!(
            keymap.holder_of(Shortcut::cmd_shift(Key::X)),
            Some(Action::SplitRight)
        );
    }

    #[test]
    fn set_drops_default_equal_override_and_tracks_deviations() {
        let mut keymap = Keymap::default();
        assert!(!keymap.deviates(Action::SplitRight));
        keymap.set(Action::SplitRight, Some(Shortcut::cmd(Key::D)));
        assert!(!keymap.deviates(Action::SplitRight));
        assert_eq!(keymap.deviations().count(), 0);

        keymap.set(Action::SplitRight, Some(Shortcut::cmd_shift(Key::X)));
        keymap.set(Action::Commit, None);
        assert!(keymap.deviates(Action::SplitRight));
        let deviations: Vec<_> = keymap.deviations().collect();
        assert_eq!(
            deviations,
            vec![
                (Action::SplitRight, Some(Shortcut::cmd_shift(Key::X))),
                (Action::Commit, None),
            ]
        );

        keymap.reset(Action::SplitRight);
        assert!(!keymap.deviates(Action::SplitRight));
        keymap.restore_defaults();
        assert_eq!(keymap, Keymap::default());
    }

    #[test]
    fn keymap_matches_requires_exact_modifiers() {
        let keymap = Keymap::default();
        assert!(keymap.matches(Action::SplitRight, Key::D, mods(true, false, false, false)));
        assert!(!keymap.matches(Action::SplitRight, Key::D, mods(true, false, false, true)));
        assert!(!keymap.matches(Action::SplitRight, Key::D, mods(false, false, false, false)));
        assert!(keymap.matches(Action::SplitDown, Key::D, mods(true, false, false, true)));
        assert!(keymap.matches(
            Action::ResizeLeft,
            Key::ArrowLeft,
            mods(true, true, false, false)
        ));
        assert!(keymap.matches(Action::Commit, Key::Enter, mods(true, false, false, false)));
    }
}
