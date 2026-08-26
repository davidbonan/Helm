use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Shadow, Stroke, Theme, Visuals,
};
use serde::{Deserialize, Serialize};

use crate::terminal::palette::TermPalette;

pub const RADIUS_PILL: u8 = 8;
pub const RADIUS_CARD: u8 = 16;
/// Context menus / dropdown frames and their item highlight — tighter than the
/// pill, macOS-menu look.
pub const RADIUS_MENU: u8 = 6;
pub const RADIUS_MENU_ITEM: u8 = 5;
/// Interactive buttons (action pills, CTAs) across the sidebars — tight,
/// near-square like the git sidebar's commit button, so buttons read
/// homogeneously wherever they appear.
pub const RADIUS_BUTTON: u8 = 4;

// Typography tokens shared across views (design-system §2; M17-18) — a size
// used by a single view stays local to it.
/// Card / panel title (git sidebar cards, diff overlay, commit detail).
pub const TITLE_SIZE: f32 = 14.0;
/// Section header inside a card (Unstaged/Staged, commit detail Files).
pub const SECTION_TITLE_SIZE: f32 = 13.0;
/// Count / action pill text.
pub const PILL_SIZE: f32 = 11.0;
/// Dimmed commit message body following the summary (graph rows M10-6,
/// commit detail) — readable body tier (nav/terminal), the dimmed 12 read too small.
pub const BODY_SIZE: f32 = 13.0;
/// Shortcut badge (`⌘1`…) shown while holding Cmd — design-system §2.
pub const SHORTCUT_BADGE_SIZE: f32 = 12.0;

const UI_FONT_PATH: &str = "/System/Library/Fonts/SFNS.ttf";
const MONO_FONT_PATH: &str = "/System/Library/Fonts/SFNSMono.ttf";
// SF (variable) only exposes its default weight via ab_glyph: the medium weight
// of titles/labels (design-system §2, D-7) comes from Helvetica Neue Medium —
// metric sibling of SF — face #10 of the system collection.
const MEDIUM_FONT_PATH: &str = "/System/Library/Fonts/HelveticaNeue.ttc";
const MEDIUM_FONT_INDEX: u32 = 10;
const MEDIUM_FAMILY: &str = "ui-medium";

// Terminal mono face: JetBrains Mono **Nerd Font Mono** (embedded, OFL —
// assets/JetBrainsMono-LICENSE, patch glyphs MIT — assets/NerdFonts-LICENSE),
// Ghostty's default face patched with the whole Nerd Font set. Same metrics as the
// unpatched JetBrains Mono (cell, ascent, descent), plus 10 900 glyphs — the E0xx/F0xx
// private-use area (powerline, agent statuslines) and braille (spinners) — every one
// of them on the mono grid, where the separate symbol fonts served them at 1.6–2.2
// cells. Its natural metrics carry a line gap SF Mono lacks, so the grid reads less
// cramped. SF Mono stays behind it as a fallback for the glyphs it covers.
const JBM_BYTES: &[u8] = include_bytes!("../assets/JetBrainsMonoNerdFontMono-Regular.ttf");
// Remaining mono fallbacks, off-grid but shrunk to their cell at paint time
// (terminal_view::fit_loose_glyph): Menlo (system, DejaVu-derived) draws the
// Dingbats — Claude Code's ✢✶✻✽ spinner, ✔✘ — as a mono face does, 1.14 cell
// against the 1.6–1.9 of Zapf Dingbats, which stays behind it for what it lacks;
// Apple Symbols (system) covers misc technical (Claude Code's ⎿) and various
// arrows. None of these blocks exist in the rest of the stack.
const MENLO_PATH: &str = "/System/Library/Fonts/Menlo.ttc";
/// Menlo.ttc faces: 0 Regular, 1 Bold, 2 Italic, 3 Bold Italic.
const MENLO_INDEX: u32 = 0;
const APPLE_SYMBOLS_PATH: &str = "/System/Library/Fonts/Apple Symbols.ttf";
const ZAPF_DINGBATS_PATH: &str = "/System/Library/Fonts/ZapfDingbats.ttf";

const ITEM_SPACING_Y: f32 = 6.0;
const PILL_PADDING_X: f32 = 10.0;
const PILL_PADDING_Y: f32 = 5.0;
const MENU_ITEM_PADDING: egui::Vec2 = egui::Vec2::new(8.0, 4.0);
const MENU_ITEM_SPACING_Y: f32 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    Auto,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Dark variant — drives mode-derived rendering (background and ink of the
    /// graph's ref chips).
    pub dark: bool,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_subtle: Color32,
    /// Identity color for AI / agent affordances (Sparkles notes, "Ask {agent}"),
    /// kept distinct from `accent` so review comments and agent prompts don't blur.
    pub accent_ai: Color32,
    pub bg_canvas: Color32,
    pub bg_sidebar: Color32,
    pub bg_surface: Color32,
    pub bg_surface_hover: Color32,
    pub border_subtle: Color32,
    pub border_input: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub state_disabled: Color32,
    pub git_added: Color32,
    pub git_modified: Color32,
    pub git_deleted: Color32,
    pub git_renamed: Color32,
    pub git_conflict: Color32,
    /// Cyclic colors of the graph lanes (M10-1) — see `lane_color`.
    pub lane_colors: [Color32; 8],
    /// Text laid over a graph node bubble (author initials, M10-3).
    pub lane_node_text: Color32,
    /// syntect theme name (two-face set) for diff coloring — the syntax follows
    /// the interface theme, not just the light/dark mode.
    pub syntax: &'static str,
}

impl Palette {
    pub const fn light() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(46, 104, 211),
            accent_hover: Color32::from_rgb(69, 121, 216),
            accent_subtle: Color32::from_rgb(234, 240, 250),
            accent_ai: Color32::from_rgb(124, 92, 224),
            bg_canvas: Color32::from_rgb(255, 255, 255),
            bg_sidebar: Color32::from_rgb(221, 222, 225),
            bg_surface: Color32::from_rgb(239, 239, 240),
            bg_surface_hover: Color32::from_rgb(246, 246, 246),
            border_subtle: Color32::from_rgb(210, 212, 217),
            border_input: Color32::from_rgb(170, 179, 197),
            text_primary: Color32::from_rgb(30, 32, 48),
            text_secondary: Color32::from_rgb(66, 69, 74),
            text_muted: Color32::from_rgb(150, 152, 156),
            state_disabled: Color32::from_rgb(145, 146, 148),
            git_added: Color32::from_rgb(36, 142, 78),
            git_modified: Color32::from_rgb(176, 120, 12),
            git_deleted: Color32::from_rgb(197, 53, 46),
            git_renamed: Color32::from_rgb(38, 130, 142),
            git_conflict: Color32::from_rgb(204, 76, 46),
            lane_colors: [
                Color32::from_rgb(37, 99, 207),
                Color32::from_rgb(173, 52, 166),
                Color32::from_rgb(176, 124, 16),
                Color32::from_rgb(33, 140, 77),
                Color32::from_rgb(200, 64, 54),
                Color32::from_rgb(23, 142, 167),
                Color32::from_rgb(107, 84, 214),
                Color32::from_rgb(199, 71, 124),
            ],
            lane_node_text: Color32::from_rgb(255, 255, 255),
            syntax: "InspiredGitHub",
        }
    }

    pub const fn dark() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(79, 134, 232),
            accent_hover: Color32::from_rgb(110, 156, 236),
            accent_subtle: Color32::from_rgb(30, 42, 64),
            accent_ai: Color32::from_rgb(157, 138, 248),
            bg_canvas: Color32::from_rgb(25, 34, 45),
            bg_sidebar: Color32::from_rgb(16, 23, 31),
            bg_surface: Color32::from_rgb(28, 37, 49),
            bg_surface_hover: Color32::from_rgb(35, 45, 59),
            border_subtle: Color32::from_rgb(41, 50, 63),
            border_input: Color32::from_rgb(58, 66, 82),
            text_primary: Color32::from_rgb(236, 236, 236),
            text_secondary: Color32::from_rgb(180, 181, 184),
            text_muted: Color32::from_rgb(138, 139, 143),
            state_disabled: Color32::from_rgb(106, 107, 110),
            git_added: Color32::from_rgb(91, 185, 126),
            git_modified: Color32::from_rgb(214, 165, 58),
            git_deleted: Color32::from_rgb(224, 108, 102),
            git_renamed: Color32::from_rgb(91, 182, 201),
            git_conflict: Color32::from_rgb(232, 131, 92),
            lane_colors: [
                Color32::from_rgb(86, 156, 245),
                Color32::from_rgb(214, 93, 207),
                Color32::from_rgb(224, 176, 62),
                Color32::from_rgb(87, 190, 122),
                Color32::from_rgb(235, 109, 98),
                Color32::from_rgb(80, 195, 218),
                Color32::from_rgb(149, 128, 245),
                Color32::from_rgb(240, 130, 170),
            ],
            lane_node_text: Color32::from_rgb(244, 245, 247),
            syntax: "base16-ocean.dark",
        }
    }

    pub const fn github_light() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(9, 105, 218),
            accent_hover: Color32::from_rgb(49, 125, 228),
            accent_subtle: Color32::from_rgb(221, 244, 255),
            accent_ai: Color32::from_rgb(130, 80, 223),
            bg_canvas: Color32::from_rgb(255, 255, 255),
            bg_sidebar: Color32::from_rgb(246, 248, 250),
            bg_surface: Color32::from_rgb(246, 248, 250),
            bg_surface_hover: Color32::from_rgb(234, 238, 242),
            border_subtle: Color32::from_rgb(208, 215, 222),
            border_input: Color32::from_rgb(175, 184, 193),
            text_primary: Color32::from_rgb(31, 35, 40),
            text_secondary: Color32::from_rgb(66, 74, 83),
            text_muted: Color32::from_rgb(101, 109, 118),
            state_disabled: Color32::from_rgb(140, 149, 159),
            git_added: Color32::from_rgb(26, 127, 55),
            git_modified: Color32::from_rgb(154, 103, 0),
            git_deleted: Color32::from_rgb(207, 34, 46),
            git_renamed: Color32::from_rgb(27, 124, 131),
            git_conflict: Color32::from_rgb(188, 76, 0),
            lane_colors: [
                Color32::from_rgb(9, 105, 218),
                Color32::from_rgb(130, 80, 223),
                Color32::from_rgb(191, 135, 0),
                Color32::from_rgb(26, 127, 55),
                Color32::from_rgb(207, 34, 46),
                Color32::from_rgb(27, 124, 131),
                Color32::from_rgb(102, 57, 186),
                Color32::from_rgb(191, 57, 137),
            ],
            lane_node_text: Color32::from_rgb(255, 255, 255),
            syntax: "GitHub",
        }
    }

    pub const fn github_dark() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(47, 129, 247),
            accent_hover: Color32::from_rgb(83, 155, 245),
            accent_subtle: Color32::from_rgb(19, 35, 57),
            accent_ai: Color32::from_rgb(163, 113, 247),
            bg_canvas: Color32::from_rgb(13, 17, 23),
            bg_sidebar: Color32::from_rgb(1, 4, 9),
            bg_surface: Color32::from_rgb(22, 27, 34),
            bg_surface_hover: Color32::from_rgb(33, 38, 45),
            border_subtle: Color32::from_rgb(48, 54, 61),
            border_input: Color32::from_rgb(72, 79, 88),
            text_primary: Color32::from_rgb(230, 237, 243),
            text_secondary: Color32::from_rgb(177, 186, 196),
            text_muted: Color32::from_rgb(125, 133, 144),
            state_disabled: Color32::from_rgb(84, 93, 104),
            git_added: Color32::from_rgb(63, 185, 80),
            git_modified: Color32::from_rgb(210, 153, 34),
            git_deleted: Color32::from_rgb(248, 81, 73),
            git_renamed: Color32::from_rgb(57, 197, 207),
            git_conflict: Color32::from_rgb(219, 109, 40),
            lane_colors: [
                Color32::from_rgb(47, 129, 247),
                Color32::from_rgb(163, 113, 247),
                Color32::from_rgb(210, 153, 34),
                Color32::from_rgb(63, 185, 80),
                Color32::from_rgb(248, 81, 73),
                Color32::from_rgb(57, 197, 207),
                Color32::from_rgb(219, 97, 162),
                Color32::from_rgb(219, 109, 40),
            ],
            lane_node_text: Color32::from_rgb(240, 246, 252),
            syntax: "Nord",
        }
    }

    pub const fn catppuccin_latte() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(30, 102, 245),
            accent_hover: Color32::from_rgb(71, 135, 247),
            accent_subtle: Color32::from_rgb(214, 224, 245),
            accent_ai: Color32::from_rgb(136, 57, 239),
            bg_canvas: Color32::from_rgb(239, 241, 245),
            bg_sidebar: Color32::from_rgb(230, 233, 239),
            bg_surface: Color32::from_rgb(220, 224, 232),
            bg_surface_hover: Color32::from_rgb(204, 208, 218),
            border_subtle: Color32::from_rgb(188, 192, 204),
            border_input: Color32::from_rgb(156, 160, 176),
            text_primary: Color32::from_rgb(76, 79, 105),
            text_secondary: Color32::from_rgb(92, 95, 119),
            text_muted: Color32::from_rgb(140, 143, 161),
            state_disabled: Color32::from_rgb(156, 160, 176),
            git_added: Color32::from_rgb(64, 160, 43),
            git_modified: Color32::from_rgb(223, 142, 29),
            git_deleted: Color32::from_rgb(210, 15, 57),
            git_renamed: Color32::from_rgb(23, 146, 153),
            git_conflict: Color32::from_rgb(254, 100, 11),
            lane_colors: [
                Color32::from_rgb(30, 102, 245),
                Color32::from_rgb(136, 57, 239),
                Color32::from_rgb(223, 142, 29),
                Color32::from_rgb(64, 160, 43),
                Color32::from_rgb(210, 15, 57),
                Color32::from_rgb(23, 146, 153),
                Color32::from_rgb(114, 135, 253),
                Color32::from_rgb(234, 118, 203),
            ],
            lane_node_text: Color32::from_rgb(255, 255, 255),
            syntax: "Catppuccin Latte",
        }
    }

    pub const fn catppuccin_mocha() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(137, 180, 250),
            accent_hover: Color32::from_rgb(166, 198, 251),
            accent_subtle: Color32::from_rgb(46, 52, 76),
            accent_ai: Color32::from_rgb(203, 166, 247),
            bg_canvas: Color32::from_rgb(30, 30, 46),
            bg_sidebar: Color32::from_rgb(24, 24, 37),
            bg_surface: Color32::from_rgb(49, 50, 68),
            bg_surface_hover: Color32::from_rgb(59, 60, 80),
            border_subtle: Color32::from_rgb(69, 71, 90),
            border_input: Color32::from_rgb(88, 91, 112),
            text_primary: Color32::from_rgb(205, 214, 244),
            text_secondary: Color32::from_rgb(186, 194, 222),
            text_muted: Color32::from_rgb(127, 132, 156),
            state_disabled: Color32::from_rgb(108, 112, 134),
            git_added: Color32::from_rgb(166, 227, 161),
            git_modified: Color32::from_rgb(249, 226, 175),
            git_deleted: Color32::from_rgb(243, 139, 168),
            git_renamed: Color32::from_rgb(148, 226, 213),
            git_conflict: Color32::from_rgb(250, 179, 135),
            lane_colors: [
                Color32::from_rgb(137, 180, 250),
                Color32::from_rgb(203, 166, 247),
                Color32::from_rgb(249, 226, 175),
                Color32::from_rgb(166, 227, 161),
                Color32::from_rgb(243, 139, 168),
                Color32::from_rgb(148, 226, 213),
                Color32::from_rgb(180, 190, 254),
                Color32::from_rgb(245, 194, 231),
            ],
            lane_node_text: Color32::from_rgb(17, 17, 27),
            syntax: "Catppuccin Mocha",
        }
    }

    pub const fn one_light() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(64, 120, 242),
            accent_hover: Color32::from_rgb(100, 147, 245),
            accent_subtle: Color32::from_rgb(226, 234, 253),
            accent_ai: Color32::from_rgb(140, 75, 214),
            bg_canvas: Color32::from_rgb(250, 250, 250),
            bg_sidebar: Color32::from_rgb(234, 234, 235),
            bg_surface: Color32::from_rgb(240, 240, 241),
            bg_surface_hover: Color32::from_rgb(229, 229, 230),
            border_subtle: Color32::from_rgb(219, 219, 220),
            border_input: Color32::from_rgb(194, 194, 195),
            text_primary: Color32::from_rgb(56, 58, 66),
            text_secondary: Color32::from_rgb(79, 82, 94),
            text_muted: Color32::from_rgb(140, 142, 151),
            state_disabled: Color32::from_rgb(160, 161, 167),
            git_added: Color32::from_rgb(80, 161, 79),
            git_modified: Color32::from_rgb(193, 132, 1),
            git_deleted: Color32::from_rgb(228, 86, 73),
            git_renamed: Color32::from_rgb(1, 132, 188),
            git_conflict: Color32::from_rgb(152, 104, 1),
            lane_colors: [
                Color32::from_rgb(64, 120, 242),
                Color32::from_rgb(166, 38, 164),
                Color32::from_rgb(193, 132, 1),
                Color32::from_rgb(80, 161, 79),
                Color32::from_rgb(228, 86, 73),
                Color32::from_rgb(1, 132, 188),
                Color32::from_rgb(152, 104, 1),
                Color32::from_rgb(202, 18, 67),
            ],
            lane_node_text: Color32::from_rgb(255, 255, 255),
            syntax: "OneHalfLight",
        }
    }

    pub const fn one_dark() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(97, 175, 239),
            accent_hover: Color32::from_rgb(132, 194, 244),
            accent_subtle: Color32::from_rgb(49, 64, 80),
            accent_ai: Color32::from_rgb(198, 120, 221),
            bg_canvas: Color32::from_rgb(40, 44, 52),
            bg_sidebar: Color32::from_rgb(33, 37, 43),
            bg_surface: Color32::from_rgb(44, 49, 58),
            bg_surface_hover: Color32::from_rgb(53, 59, 69),
            border_subtle: Color32::from_rgb(62, 68, 81),
            border_input: Color32::from_rgb(75, 82, 99),
            text_primary: Color32::from_rgb(171, 178, 191),
            text_secondary: Color32::from_rgb(157, 165, 180),
            text_muted: Color32::from_rgb(92, 99, 112),
            state_disabled: Color32::from_rgb(75, 82, 99),
            git_added: Color32::from_rgb(152, 195, 121),
            git_modified: Color32::from_rgb(229, 192, 123),
            git_deleted: Color32::from_rgb(224, 108, 117),
            git_renamed: Color32::from_rgb(86, 182, 194),
            git_conflict: Color32::from_rgb(209, 154, 102),
            lane_colors: [
                Color32::from_rgb(97, 175, 239),
                Color32::from_rgb(198, 120, 221),
                Color32::from_rgb(229, 192, 123),
                Color32::from_rgb(152, 195, 121),
                Color32::from_rgb(224, 108, 117),
                Color32::from_rgb(86, 182, 194),
                Color32::from_rgb(209, 154, 102),
                Color32::from_rgb(190, 80, 70),
            ],
            lane_node_text: Color32::from_rgb(40, 44, 52),
            syntax: "OneHalfDark",
        }
    }

    pub const fn tokyo_day() -> Self {
        Self {
            dark: false,
            accent: Color32::from_rgb(46, 125, 233),
            accent_hover: Color32::from_rgb(88, 150, 238),
            accent_subtle: Color32::from_rgb(204, 214, 231),
            accent_ai: Color32::from_rgb(120, 71, 189),
            bg_canvas: Color32::from_rgb(225, 226, 231),
            bg_sidebar: Color32::from_rgb(216, 217, 224),
            bg_surface: Color32::from_rgb(220, 221, 228),
            bg_surface_hover: Color32::from_rgb(196, 200, 218),
            border_subtle: Color32::from_rgb(168, 174, 203),
            border_input: Color32::from_rgb(153, 160, 191),
            text_primary: Color32::from_rgb(55, 96, 191),
            text_secondary: Color32::from_rgb(97, 114, 176),
            text_muted: Color32::from_rgb(132, 140, 181),
            state_disabled: Color32::from_rgb(161, 166, 197),
            git_added: Color32::from_rgb(88, 117, 57),
            git_modified: Color32::from_rgb(140, 108, 62),
            git_deleted: Color32::from_rgb(245, 42, 101),
            git_renamed: Color32::from_rgb(0, 113, 151),
            git_conflict: Color32::from_rgb(177, 92, 0),
            lane_colors: [
                Color32::from_rgb(46, 125, 233),
                Color32::from_rgb(152, 84, 241),
                Color32::from_rgb(140, 108, 62),
                Color32::from_rgb(88, 117, 57),
                Color32::from_rgb(245, 42, 101),
                Color32::from_rgb(0, 113, 151),
                Color32::from_rgb(120, 71, 189),
                Color32::from_rgb(177, 92, 0),
            ],
            lane_node_text: Color32::from_rgb(255, 255, 255),
            syntax: "Coldark-Cold",
        }
    }

    pub const fn tokyo_night() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(122, 162, 247),
            accent_hover: Color32::from_rgb(154, 184, 249),
            accent_subtle: Color32::from_rgb(40, 47, 69),
            accent_ai: Color32::from_rgb(187, 154, 247),
            bg_canvas: Color32::from_rgb(26, 27, 38),
            bg_sidebar: Color32::from_rgb(22, 22, 30),
            bg_surface: Color32::from_rgb(31, 34, 49),
            bg_surface_hover: Color32::from_rgb(41, 46, 66),
            border_subtle: Color32::from_rgb(47, 53, 73),
            border_input: Color32::from_rgb(59, 66, 97),
            text_primary: Color32::from_rgb(192, 202, 245),
            text_secondary: Color32::from_rgb(169, 177, 214),
            text_muted: Color32::from_rgb(86, 95, 137),
            state_disabled: Color32::from_rgb(65, 72, 104),
            git_added: Color32::from_rgb(158, 206, 106),
            git_modified: Color32::from_rgb(224, 175, 104),
            git_deleted: Color32::from_rgb(247, 118, 142),
            git_renamed: Color32::from_rgb(125, 207, 255),
            git_conflict: Color32::from_rgb(255, 158, 100),
            lane_colors: [
                Color32::from_rgb(122, 162, 247),
                Color32::from_rgb(187, 154, 247),
                Color32::from_rgb(224, 175, 104),
                Color32::from_rgb(158, 206, 106),
                Color32::from_rgb(247, 118, 142),
                Color32::from_rgb(125, 207, 255),
                Color32::from_rgb(157, 124, 216),
                Color32::from_rgb(255, 158, 100),
            ],
            lane_node_text: Color32::from_rgb(21, 22, 30),
            syntax: "Nord",
        }
    }

    /// Color of graph lane `lane`, cyclic beyond the palette (stable across
    /// pagination: `assign_lanes` is forward-only).
    pub fn lane_color(&self, lane: usize) -> Color32 {
        self.lane_colors[lane % self.lane_colors.len()]
    }

    /// Fill of the solid primary button (Commit, Open Folder…): `accent`
    /// darkened one notch — the token stays full color for its other consumers
    /// (links, active tabs, lanes). Dark presets' accents are brighter to begin
    /// with ⇒ a stronger factor in dark mode.
    pub fn primary_button_fill(&self) -> Color32 {
        darken(self.accent, self.primary_button_darken())
    }

    pub fn primary_button_hover(&self) -> Color32 {
        darken(self.accent_hover, self.primary_button_darken())
    }

    fn primary_button_darken(&self) -> f32 {
        if self.dark {
            PRIMARY_BUTTON_DARKEN_DARK
        } else {
            PRIMARY_BUTTON_DARKEN_LIGHT
        }
    }
}

const PRIMARY_BUTTON_DARKEN_LIGHT: f32 = 0.85;
const PRIMARY_BUTTON_DARKEN_DARK: f32 = 0.70;

fn darken(color: Color32, factor: f32) -> Color32 {
    let [r, g, b, _] = color.to_srgba_unmultiplied();
    let d = |c: u8| (c as f32 * factor) as u8;
    Color32::from_rgb(d(r), d(g), d(b))
}

/// Installs the macOS system fonts (SF Pro UI / SF Mono) at the head of the egui
/// families, plus the Lucide icon font (embedded by the `lucide-icons` crate,
/// Unicode private-use glyphs) as a fallback of the proportional family. egui's
/// bundled fonts stay behind SF, for the glyphs SF doesn't cover. Call once at
/// startup. An unreadable or unparseable system font is skipped — Lucide,
/// embedded, is always installed.
pub fn install_fonts(ctx: &egui::Context) {
    ctx.set_fonts(font_definitions());
}

pub fn font_definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    if let Some(data) = load_font(UI_FONT_PATH) {
        register_font(&mut fonts, "sf-pro", data, FontFamily::Proportional);
    }
    // Head insertions: final order jetbrains-mono → sf-mono → menlo →
    // apple-symbols → zapf-dingbats → egui fonts. The symbol fallbacks come before
    // Hack (egui), whose powerline glyphs have metrics foreign to the mono face.
    if let Some(data) = load_font(ZAPF_DINGBATS_PATH) {
        register_font(&mut fonts, "zapf-dingbats", data, FontFamily::Monospace);
    }
    if let Some(data) = load_font(APPLE_SYMBOLS_PATH) {
        register_font(&mut fonts, "apple-symbols", data, FontFamily::Monospace);
    }
    if let Some(data) = load_font_face(MENLO_PATH, MENLO_INDEX) {
        register_font(&mut fonts, "menlo", data, FontFamily::Monospace);
    }
    if let Some(data) = load_font(MONO_FONT_PATH) {
        register_font(&mut fonts, "sf-mono", data, FontFamily::Monospace);
    }
    register_font(
        &mut fonts,
        "jetbrains-mono",
        FontData::from_static(JBM_BYTES),
        FontFamily::Monospace,
    );
    fonts.font_data.insert(
        "lucide".to_owned(),
        Arc::new(FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES)),
    );
    // Lucide (the app's icon set) backs the proportional family right after SF
    // Pro, ahead of the symbol fallbacks: it shares the private-use range with the
    // Nerd Font face, so the app icons must win there. The symbol fallbacks follow (tab
    // titles render proportional, and a process e.g. Claude Code can emit Nerd Font
    // glyphs in its OSC title Lucide doesn't cover); both sit before egui's bundled
    // fonts, which only fill glyphs SF Pro lacks. Normal text keeps SF Pro.
    let symbol_fallbacks: Vec<String> =
        ["jetbrains-mono", "menlo", "apple-symbols", "zapf-dingbats"]
            .into_iter()
            .filter(|name| fonts.font_data.contains_key(*name))
            .map(str::to_owned)
            .collect();
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(1, "lucide".to_owned());
    for (i, name) in symbol_fallbacks.into_iter().enumerate() {
        proportional.insert(2 + i, name);
    }
    // Medium family: the medium face at the head, then the whole proportional
    // stack as fallback (regular text if the face is missing, Lucide icons and
    // uncovered glyphs always rendered).
    let mut medium: Vec<String> = Vec::new();
    if let Some(data) = load_font_face(MEDIUM_FONT_PATH, MEDIUM_FONT_INDEX) {
        fonts
            .font_data
            .insert("hn-medium".to_owned(), Arc::new(data));
        medium.push("hn-medium".to_owned());
    }
    medium.extend(fonts.families[&FontFamily::Proportional].iter().cloned());
    fonts
        .families
        .insert(FontFamily::Name(MEDIUM_FAMILY.into()), medium);
    fonts
}

/// **Medium** proportional family (~weight 500) for titles and emphatic labels.
/// Only reference it through this helper: a context that hasn't received
/// [`install_fonts`] (kittest harness) doesn't know the family and epaint panics
/// on an unknown family — we then fall back to the regular proportional.
pub fn medium_family(ctx: &egui::Context) -> FontFamily {
    let family = FontFamily::Name(MEDIUM_FAMILY.into());
    if ctx.fonts(|f| f.definitions().families.contains_key(&family)) {
        family
    } else {
        FontFamily::Proportional
    }
}

fn register_font(fonts: &mut FontDefinitions, name: &str, data: FontData, family: FontFamily) {
    fonts.font_data.insert(name.to_owned(), Arc::new(data));
    fonts
        .families
        .entry(family)
        .or_default()
        .insert(0, name.to_owned());
}

fn load_font(path: &str) -> Option<FontData> {
    let bytes = std::fs::read(path).ok()?;
    ab_glyph::FontRef::try_from_slice(&bytes).ok()?;
    Some(FontData::from_owned(bytes))
}

/// Loads a specific face from a collection (`.ttc`) — same validation as
/// [`load_font`], modulo the index.
fn load_font_face(path: &str, index: u32) -> Option<FontData> {
    let bytes = std::fs::read(path).ok()?;
    ab_glyph::FontRef::try_from_slice_and_index(&bytes, index).ok()?;
    let mut data = FontData::from_owned(bytes);
    data.index = index;
    Some(data)
}

/// A selectable theme: matching chrome palette + terminal palette (a single
/// choice recolors the whole interface, terminal and diff included). The same
/// family `id` covers the light and dark variants — the prefs store one id per
/// mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemePreset {
    pub id: &'static str,
    pub name: &'static str,
    pub dark: bool,
    pub palette: Palette,
    pub term: TermPalette,
}

/// Embedded themes, in light/dark pairs. Helm (first) is the default.
pub const PRESETS: [ThemePreset; 10] = [
    ThemePreset {
        id: "helm",
        name: "Helm",
        dark: false,
        palette: Palette::light(),
        term: TermPalette::light(),
    },
    ThemePreset {
        id: "helm",
        name: "Helm",
        dark: true,
        palette: Palette::dark(),
        term: TermPalette::dark(),
    },
    ThemePreset {
        id: "github",
        name: "GitHub Light",
        dark: false,
        palette: Palette::github_light(),
        term: TermPalette::github_light(),
    },
    ThemePreset {
        id: "github",
        name: "GitHub Dark",
        dark: true,
        palette: Palette::github_dark(),
        term: TermPalette::github_dark(),
    },
    ThemePreset {
        id: "catppuccin",
        name: "Catppuccin Latte",
        dark: false,
        palette: Palette::catppuccin_latte(),
        term: TermPalette::catppuccin_latte(),
    },
    ThemePreset {
        id: "catppuccin",
        name: "Catppuccin Mocha",
        dark: true,
        palette: Palette::catppuccin_mocha(),
        term: TermPalette::catppuccin_mocha(),
    },
    ThemePreset {
        id: "one",
        name: "One Light",
        dark: false,
        palette: Palette::one_light(),
        term: TermPalette::one_light(),
    },
    ThemePreset {
        id: "one",
        name: "One Dark",
        dark: true,
        palette: Palette::one_dark(),
        term: TermPalette::one_dark(),
    },
    ThemePreset {
        id: "tokyo",
        name: "Tokyo Night Day",
        dark: false,
        palette: Palette::tokyo_day(),
        term: TermPalette::tokyo_day(),
    },
    ThemePreset {
        id: "tokyo",
        name: "Tokyo Night",
        dark: true,
        palette: Palette::tokyo_night(),
        term: TermPalette::tokyo_night(),
    },
];

/// Preset of the requested variant. An unknown id (hand-edited prefs, removed
/// theme) falls back to Helm — the app stays usable without rewriting the TOML.
pub fn preset(id: &str, dark: bool) -> &'static ThemePreset {
    PRESETS
        .iter()
        .find(|p| p.id == id && p.dark == dark)
        .unwrap_or(&PRESETS[dark as usize])
}

/// Helm palette for the mode — shortcut for surfaces that don't depend on the
/// theme choice (tests, components driven outside the app).
pub fn palette(theme: Theme) -> Palette {
    match theme {
        Theme::Light => Palette::light(),
        Theme::Dark => Palette::dark(),
    }
}

pub fn resolve(mode: ThemeMode, system: Theme) -> Theme {
    match mode {
        ThemeMode::Auto => system,
        ThemeMode::Light => Theme::Light,
        ThemeMode::Dark => Theme::Dark,
    }
}

/// Resolves mode + theme selection, pushes the egui visuals, and returns the
/// active preset — the app derives chrome palette and terminal palette from it.
pub fn apply(
    ctx: &egui::Context,
    mode: ThemeMode,
    light: &str,
    dark: &str,
) -> &'static ThemePreset {
    let system = ctx.system_theme().unwrap_or(Theme::Light);
    let theme = resolve(mode, system);
    let preset = match theme {
        Theme::Light => preset(light, false),
        Theme::Dark => preset(dark, true),
    };
    ctx.set_visuals(visuals(theme, &preset.palette));
    ctx.global_style_mut(apply_spacing);
    preset
}

fn apply_spacing(style: &mut egui::Style) {
    let s = &mut style.spacing;
    s.item_spacing = egui::vec2(s.item_spacing.x, ITEM_SPACING_Y);
    s.button_padding = egui::vec2(PILL_PADDING_X, PILL_PADDING_Y);
    // Pointer cursor on native `egui::Button`s (menus, modals); custom widgets
    // go through `ui::clickable`.
    style.visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    // The graph virtualizes its rows (fixed-height, only the visible slice is
    // allocated), so a fixed screen slot legitimately holds a different commit
    // each frame while scrolling. egui's debug-only `warn_if_rect_changes_id`
    // (a red overlay drawn in debug builds) reads that rect reuse as a bug and
    // flashes at the top of the list on a fast scroll — a false positive for any
    // virtualized list. Off so dev builds don't show the spurious box.
    // `Style::debug` only exists under `debug_assertions`; release omits the field.
    #[cfg(debug_assertions)]
    {
        style.debug.warn_if_rect_changes_id = false;
    }
}

fn visuals(theme: Theme, p: &Palette) -> Visuals {
    let mut v = theme.default_visuals();
    v.override_text_color = Some(p.text_primary);
    v.hyperlink_color = p.accent;
    v.panel_fill = p.bg_canvas;
    v.window_fill = p.bg_canvas;
    v.window_stroke = Stroke::new(1.0_f32, p.border_subtle);
    v.extreme_bg_color = p.bg_surface;
    v.faint_bg_color = p.bg_surface_hover;
    v.code_bg_color = p.bg_surface;
    v.selection.bg_fill = p.accent_subtle;
    v.selection.stroke = Stroke::new(1.0_f32, p.accent);
    v.window_corner_radius = CornerRadius::same(RADIUS_CARD);
    v.menu_corner_radius = CornerRadius::same(RADIUS_MENU);
    // Menus/dropdowns: diffuse centered shadow (egui's default is an offset
    // [6,10] drop shadow) — the 1px `window_stroke` does the separation work.
    v.popup_shadow = Shadow {
        offset: [0, 2],
        blur: 12,
        spread: 0,
        color: Color32::from_black_alpha(if p.dark { 96 } else { 28 }),
    };

    let w = &mut v.widgets;
    for ws in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        ws.corner_radius = CornerRadius::same(RADIUS_PILL);
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_secondary);
    w.inactive.bg_fill = p.bg_surface;
    w.inactive.weak_bg_fill = p.bg_surface;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.border_subtle);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_secondary);
    w.hovered.bg_fill = p.bg_surface_hover;
    w.hovered.weak_bg_fill = p.bg_surface_hover;
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_input);
    w.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    w.active.bg_fill = p.accent;
    w.active.weak_bg_fill = p.accent;
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent_hover);
    w.active.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    v
}

/// Content style of menus and dropdowns (`Popup::style`): egui's `menu_style`
/// (strips the item strokes and the inactive fill) + flat neutral highlight —
/// comfortable padding, tight rows, [`RADIUS_MENU_ITEM`] rounding, and a press
/// state kept on the hover grey instead of the accent.
pub fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = MENU_ITEM_PADDING;
    style.spacing.item_spacing.y = MENU_ITEM_SPACING_Y;
    let w = &mut style.visuals.widgets;
    let hover_fill = w.hovered.weak_bg_fill;
    w.active.weak_bg_fill = hover_fill;
    w.open.weak_bg_fill = hover_fill;
    for ws in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        ws.corner_radius = CornerRadius::same(RADIUS_MENU_ITEM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_and_dark_palettes_differ() {
        let light = Palette::light();
        let dark = Palette::dark();
        assert_ne!(light, dark);
        assert_ne!(light.bg_canvas, dark.bg_canvas);
        assert_ne!(light.text_primary, dark.text_primary);
        assert_ne!(light.accent, dark.accent);
    }

    #[test]
    fn every_preset_pairs_its_chrome_and_terminal_on_one_background() {
        // A preset recolors chrome and terminal together, on the same base. The Agents
        // columns view leans on it: it fills the gutter beside a mirrored pane with
        // `bg_canvas`, which must be the pane's own background whichever preset is on.
        for p in PRESETS {
            let (c, t) = (p.palette.bg_canvas, p.term.background);
            assert_eq!(
                (c.r(), c.g(), c.b()),
                (t.r, t.g, t.b),
                "preset {} ({}) splits its background",
                p.id,
                if p.dark { "dark" } else { "light" }
            );
        }
    }

    #[test]
    fn lane_colors_are_distinct_within_each_palette() {
        for p in [Palette::light(), Palette::dark()] {
            for (i, a) in p.lane_colors.iter().enumerate() {
                for b in p.lane_colors.iter().skip(i + 1) {
                    assert_ne!(a, b);
                }
            }
        }
        assert_ne!(Palette::light().lane_colors, Palette::dark().lane_colors);
    }

    #[test]
    fn lane_color_cycles_beyond_the_palette() {
        let p = Palette::dark();
        assert_eq!(p.lane_color(0), p.lane_colors[0]);
        assert_eq!(p.lane_color(8), p.lane_colors[0]);
        assert_eq!(p.lane_color(11), p.lane_colors[3]);
    }

    #[test]
    fn primary_button_fill_darkens_the_accent_per_mode() {
        // ×0.85 in light, ×0.70 in dark (design-system §4).
        let light = Palette::light();
        assert_eq!(light.primary_button_fill(), Color32::from_rgb(39, 88, 179));
        assert_eq!(
            light.primary_button_hover(),
            Color32::from_rgb(58, 102, 183)
        );
        let dark = Palette::dark();
        assert_eq!(dark.primary_button_fill(), Color32::from_rgb(55, 93, 162));
        assert_eq!(dark.primary_button_hover(), Color32::from_rgb(77, 109, 165));
    }

    #[test]
    fn light_tokens_match_design_system() {
        let p = Palette::light();
        assert_eq!(p.accent, Color32::from_rgb(46, 104, 211));
        assert_eq!(p.bg_sidebar, Color32::from_rgb(221, 222, 225));
        assert_eq!(p.text_muted, Color32::from_rgb(150, 152, 156));
    }

    #[test]
    fn dark_tokens_match_design_system_navy() {
        let p = Palette::dark();
        assert_eq!(p.bg_canvas, Color32::from_rgb(25, 34, 45));
        assert_eq!(p.bg_sidebar, Color32::from_rgb(16, 23, 31));
        assert_eq!(p.bg_surface, Color32::from_rgb(28, 37, 49));
        assert_eq!(p.bg_surface_hover, Color32::from_rgb(35, 45, 59));
        assert_eq!(p.border_subtle, Color32::from_rgb(41, 50, 63));
    }

    #[test]
    fn resolve_auto_follows_system() {
        assert_eq!(resolve(ThemeMode::Auto, Theme::Dark), Theme::Dark);
        assert_eq!(resolve(ThemeMode::Auto, Theme::Light), Theme::Light);
    }

    #[test]
    fn resolve_forced_modes_ignore_system() {
        assert_eq!(resolve(ThemeMode::Light, Theme::Dark), Theme::Light);
        assert_eq!(resolve(ThemeMode::Dark, Theme::Light), Theme::Dark);
    }

    #[test]
    fn visuals_carry_design_system_radii() {
        let v = visuals(Theme::Light, &Palette::light());
        assert_eq!(
            v.widgets.inactive.corner_radius,
            CornerRadius::same(RADIUS_PILL)
        );
        assert_eq!(
            v.widgets.hovered.corner_radius,
            CornerRadius::same(RADIUS_PILL)
        );
        assert_eq!(v.window_corner_radius, CornerRadius::same(RADIUS_CARD));
        assert_eq!(v.menu_corner_radius, CornerRadius::same(RADIUS_MENU));
    }

    #[test]
    fn popup_shadow_is_centered_and_subtle() {
        let light = visuals(Theme::Light, &Palette::light());
        assert_eq!(light.popup_shadow.offset, [0, 2]);
        assert_eq!(light.popup_shadow.blur, 12);
        let dark = visuals(Theme::Dark, &Palette::dark());
        assert_eq!(dark.popup_shadow.offset, [0, 2]);
        assert!(
            dark.popup_shadow.color.a() > light.popup_shadow.color.a(),
            "dark menus need a stronger shadow to detach"
        );
    }

    #[test]
    fn menu_style_flattens_the_item_highlight() {
        let mut style = egui::Style {
            visuals: visuals(Theme::Light, &Palette::light()),
            ..Default::default()
        };
        menu_style(&mut style);
        let w = &style.visuals.widgets;
        assert_eq!(
            w.hovered.corner_radius,
            CornerRadius::same(RADIUS_MENU_ITEM)
        );
        assert_eq!(w.hovered.bg_stroke, Stroke::NONE);
        assert_eq!(w.inactive.weak_bg_fill, Color32::TRANSPARENT);
        assert_eq!(
            w.active.weak_bg_fill, w.hovered.weak_bg_fill,
            "press stays on the hover grey, not the accent"
        );
        assert_eq!(style.spacing.button_padding, MENU_ITEM_PADDING);
        assert_eq!(style.spacing.item_spacing.y, MENU_ITEM_SPACING_Y);
    }

    #[test]
    fn spacing_matches_design_system() {
        let mut style = egui::Style::default();
        apply_spacing(&mut style);
        assert_eq!(
            style.spacing.button_padding,
            egui::vec2(PILL_PADDING_X, PILL_PADDING_Y)
        );
        assert_eq!(style.spacing.item_spacing.y, ITEM_SPACING_Y);
    }

    #[test]
    fn apply_uses_the_selected_preset_for_the_resolved_mode() {
        let ctx = egui::Context::default();
        let preset = apply(&ctx, ThemeMode::Dark, "helm", "github");
        assert_eq!(preset.name, "GitHub Dark");
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            Palette::github_dark().bg_canvas
        );

        let preset = apply(&ctx, ThemeMode::Light, "tokyo", "github");
        assert_eq!(preset.name, "Tokyo Night Day");
        assert_eq!(
            ctx.global_style().visuals.panel_fill,
            Palette::tokyo_day().bg_canvas
        );
    }

    #[test]
    fn the_registry_pairs_every_family_in_light_and_dark() {
        let light: Vec<&str> = PRESETS.iter().filter(|p| !p.dark).map(|p| p.id).collect();
        let dark: Vec<&str> = PRESETS.iter().filter(|p| p.dark).map(|p| p.id).collect();
        assert_eq!(light, dark, "every family exists in light and dark");
        assert_eq!(light, ["helm", "github", "catppuccin", "one", "tokyo"]);
    }

    #[test]
    fn preset_lookup_finds_the_variant_and_falls_back_to_helm() {
        assert_eq!(preset("github", true).name, "GitHub Dark");
        assert_eq!(preset("github", false).name, "GitHub Light");
        assert_eq!(preset("does-not-exist", false).name, "Helm");
        assert_eq!(preset("does-not-exist", true).name, "Helm");
        assert!(preset("does-not-exist", true).dark);
    }

    #[test]
    fn preset_palettes_carry_their_mode() {
        for p in &PRESETS {
            assert_eq!(p.palette.dark, p.dark, "{}", p.name);
        }
    }

    #[test]
    fn presets_keep_text_readable_on_their_canvas() {
        for p in &PRESETS {
            assert_ne!(
                p.palette.text_primary, p.palette.bg_canvas,
                "{}: invisible text",
                p.name
            );
            assert_ne!(
                p.palette.accent, p.palette.bg_canvas,
                "{}: invisible accent",
                p.name
            );
        }
    }

    #[test]
    fn load_font_returns_none_for_a_missing_path() {
        assert!(load_font("/no/such/font-file.ttf").is_none());
    }

    #[test]
    fn load_font_rejects_a_readable_non_font() {
        // Read from the crate root during tests: exists but is not a font.
        assert!(load_font("Cargo.toml").is_none());
    }

    #[test]
    fn the_medium_family_is_registered_with_the_proportional_stack_as_fallback() {
        let fonts = font_definitions();
        let medium = &fonts.families[&FontFamily::Name(MEDIUM_FAMILY.into())];
        assert!(
            medium.ends_with(&fonts.families[&FontFamily::Proportional]),
            "the whole proportional stack must stay as fallback of the medium family"
        );
    }

    #[test]
    fn medium_family_falls_back_to_proportional_without_install_fonts() {
        let ctx = egui::Context::default();
        // Force initialization of the default fonts (no install_fonts).
        let _ = ctx.run_ui(Default::default(), |_| {});
        assert_eq!(medium_family(&ctx), FontFamily::Proportional);
    }

    #[test]
    fn medium_family_resolves_after_install_fonts() {
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        let _ = ctx.run_ui(Default::default(), |_| {});
        assert_eq!(
            medium_family(&ctx),
            FontFamily::Name(MEDIUM_FAMILY.into()),
            "after install_fonts the medium family must be served"
        );
    }

    #[test]
    fn the_nerd_font_face_leads_the_mono_family_ahead_of_the_symbol_fallbacks() {
        let fonts = font_definitions();
        let mono = &fonts.families[&FontFamily::Monospace];
        assert!(fonts.font_data.contains_key("jetbrains-mono"));
        let jbm = mono
            .iter()
            .position(|n| n == "jetbrains-mono")
            .expect("JetBrains Mono Nerd Font is the primary mono");
        assert_eq!(
            jbm, 0,
            "the Nerd Font face serves the private-use area first"
        );
        if let Some(sf) = mono.iter().position(|n| n == "sf-mono") {
            assert!(jbm < sf, "JetBrains Mono outranks SF Mono");
        }
        let hack = mono.iter().position(|n| n == "Hack").unwrap();
        for fallback in ["menlo", "apple-symbols", "zapf-dingbats"] {
            if let Some(pos) = mono.iter().position(|n| n == fallback) {
                assert!(jbm < pos, "the mono face comes before {fallback}");
                assert!(
                    pos < hack,
                    "the symbol fallbacks come before the egui fonts (Hack's powerline)"
                );
            }
        }
    }

    #[test]
    fn the_bundled_face_serves_powerline_and_braille_on_the_mono_grid() {
        let data = &font_definitions().font_data["jetbrains-mono"];
        let face = ab_glyph::FontRef::try_from_slice(&data.font).unwrap();
        let scaled = <_ as ab_glyph::Font>::as_scaled(&face, 14.0_f32);
        let cell = ab_glyph::ScaleFont::h_advance(&scaled, ab_glyph::Font::glyph_id(&face, 'M'));
        // The glyphs the separate symbol fonts used to serve off-grid: powerline,
        // a Nerd Font icon, braille (spinners), box drawing.
        for c in [
            '\u{e0a0}', '\u{e0b0}', '\u{f015}', '\u{280b}', '\u{28ff}', '\u{2500}',
        ] {
            let gid = ab_glyph::Font::glyph_id(&face, c);
            assert!(
                gid.0 != 0,
                "U+{:04X} missing from the bundled face",
                c as u32
            );
            let advance = ab_glyph::ScaleFont::h_advance(&scaled, gid);
            assert!(
                (advance - cell).abs() < 0.01,
                "U+{:04X} advance {advance} ≠ cell {cell}",
                c as u32
            );
        }
    }

    #[test]
    fn menlo_serves_the_dingbats_ahead_of_zapf_and_closer_to_the_cell() {
        let fonts = font_definitions();
        let mono = &fonts.families[&FontFamily::Monospace];
        let Some(menlo) = mono.iter().position(|n| n == "menlo") else {
            return; // system font absent (CI image): the stack simply falls back
        };
        if let Some(zapf) = mono.iter().position(|n| n == "zapf-dingbats") {
            assert!(
                menlo < zapf,
                "Menlo draws the Dingbats closer to the mono cell"
            );
        }
        let advances = |name: &str, chars: &[char]| -> Vec<f32> {
            let data = &fonts.font_data[name];
            let face = ab_glyph::FontRef::try_from_slice_and_index(&data.font, data.index).unwrap();
            let scaled = <_ as ab_glyph::Font>::as_scaled(&face, 14.0_f32);
            chars
                .iter()
                .map(|c| {
                    ab_glyph::ScaleFont::h_advance(&scaled, ab_glyph::Font::glyph_id(&face, *c))
                })
                .collect()
        };
        let grid = advances("jetbrains-mono", &['M'])[0];
        // Claude Code's spinner and its ✔/✘, off the grid in every fallback — Menlo
        // stays within a shrink the eye doesn't catch, where Zapf reached 1.9 cell.
        let dingbats = [
            '\u{273B}', '\u{273D}', '\u{2722}', '\u{2733}', '\u{2714}', '\u{2718}',
        ];
        for (c, advance) in dingbats.iter().zip(advances("menlo", &dingbats)) {
            let ratio = advance / grid;
            assert!(
                (1.0..1.2).contains(&ratio),
                "U+{:04X} at {ratio:.2} cell in Menlo",
                *c as u32
            );
        }
    }

    #[test]
    fn lucide_outranks_the_symbol_fallbacks_in_the_proportional_family() {
        let fonts = font_definitions();
        assert!(fonts.font_data.contains_key("lucide"));
        let proportional = &fonts.families[&FontFamily::Proportional];
        let lucide = proportional.iter().position(|n| n == "lucide").unwrap();
        assert!(
            lucide > 0,
            "a text font must stay ahead of lucide so normal text never hits the icon font"
        );
        for name in ["jetbrains-mono", "menlo", "apple-symbols", "zapf-dingbats"] {
            if let Some(p) = proportional.iter().position(|n| n == name) {
                assert!(
                    lucide < p,
                    "lucide and the Nerd Font face share the private-use range: \
                     the app icon set must outrank {name}"
                );
            }
        }
    }
}
