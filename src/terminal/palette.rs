use alacritty_terminal::vte::ansi::{Color, NamedColor};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

const DIM_INK_PERCENT: u16 = 55;
const DIM_BG_PERCENT: u16 = 100 - DIM_INK_PERCENT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TermTheme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TermPalette {
    pub background: Rgb,
    pub foreground: Rgb,
    pub selection: Rgb,
    ansi: [Rgb; 16],
}

impl TermPalette {
    pub const fn dark() -> Self {
        Self {
            background: Rgb::new(0x19, 0x22, 0x2D),
            foreground: Rgb::new(0xEC, 0xEC, 0xEC),
            selection: Rgb::new(0x34, 0x46, 0x66),
            ansi: [
                Rgb::new(0x23, 0x2D, 0x3B),
                Rgb::new(0xE5, 0x48, 0x4D),
                Rgb::new(0x46, 0xA7, 0x58),
                Rgb::new(0xE2, 0xA0, 0x3F),
                Rgb::new(0x4F, 0x86, 0xE8),
                Rgb::new(0xB0, 0x5C, 0xCC),
                Rgb::new(0x38, 0xA3, 0xA5),
                Rgb::new(0xC9, 0xCA, 0xCC),
                Rgb::new(0x5A, 0x63, 0x74),
                Rgb::new(0xFF, 0x63, 0x69),
                Rgb::new(0x5D, 0xC9, 0x71),
                Rgb::new(0xF2, 0xC5, 0x5C),
                Rgb::new(0x6E, 0x9C, 0xEC),
                Rgb::new(0xC7, 0x7D, 0xDB),
                Rgb::new(0x4F, 0xC3, 0xC5),
                Rgb::new(0xEC, 0xEC, 0xEC),
            ],
        }
    }

    pub const fn light() -> Self {
        Self {
            background: Rgb::new(0xFF, 0xFF, 0xFF),
            foreground: Rgb::new(0x1E, 0x20, 0x30),
            selection: Rgb::new(0xBE, 0xD6, 0xF7),
            ansi: [
                Rgb::new(0x1E, 0x20, 0x30),
                Rgb::new(0xC4, 0x2B, 0x2B),
                Rgb::new(0x29, 0x7A, 0x3A),
                Rgb::new(0x9A, 0x67, 0x00),
                Rgb::new(0x2E, 0x68, 0xD3),
                Rgb::new(0x8E, 0x44, 0xAD),
                Rgb::new(0x1F, 0x7A, 0x8C),
                Rgb::new(0xD2, 0xD4, 0xD9),
                Rgb::new(0x6A, 0x6B, 0x6E),
                Rgb::new(0xE5, 0x48, 0x4D),
                Rgb::new(0x46, 0xA7, 0x58),
                Rgb::new(0xB7, 0x79, 0x1F),
                Rgb::new(0x45, 0x79, 0xD8),
                Rgb::new(0xA6, 0x5B, 0xC2),
                Rgb::new(0x2A, 0x93, 0xA6),
                Rgb::new(0xFF, 0xFF, 0xFF),
            ],
        }
    }

    pub const fn github_dark() -> Self {
        Self {
            background: Rgb::new(0x0D, 0x11, 0x17),
            foreground: Rgb::new(0xE6, 0xED, 0xF3),
            selection: Rgb::new(0x26, 0x4F, 0x78),
            ansi: [
                Rgb::new(0x48, 0x4F, 0x58),
                Rgb::new(0xFF, 0x7B, 0x72),
                Rgb::new(0x3F, 0xB9, 0x50),
                Rgb::new(0xD2, 0x99, 0x22),
                Rgb::new(0x58, 0xA6, 0xFF),
                Rgb::new(0xBC, 0x8C, 0xFF),
                Rgb::new(0x39, 0xC5, 0xCF),
                Rgb::new(0xB1, 0xBA, 0xC4),
                Rgb::new(0x6E, 0x76, 0x81),
                Rgb::new(0xFF, 0xA1, 0x98),
                Rgb::new(0x56, 0xD3, 0x64),
                Rgb::new(0xE3, 0xB3, 0x41),
                Rgb::new(0x79, 0xC0, 0xFF),
                Rgb::new(0xD2, 0xA8, 0xFF),
                Rgb::new(0x56, 0xD4, 0xDD),
                Rgb::new(0xF0, 0xF6, 0xFC),
            ],
        }
    }

    pub const fn github_light() -> Self {
        Self {
            background: Rgb::new(0xFF, 0xFF, 0xFF),
            foreground: Rgb::new(0x1F, 0x23, 0x28),
            selection: Rgb::new(0xAD, 0xD6, 0xFF),
            ansi: [
                Rgb::new(0x24, 0x29, 0x2F),
                Rgb::new(0xCF, 0x22, 0x2E),
                Rgb::new(0x11, 0x63, 0x29),
                Rgb::new(0x4D, 0x2D, 0x00),
                Rgb::new(0x09, 0x69, 0xDA),
                Rgb::new(0x82, 0x50, 0xDF),
                Rgb::new(0x1B, 0x7C, 0x83),
                Rgb::new(0x6E, 0x77, 0x81),
                Rgb::new(0x57, 0x60, 0x6A),
                Rgb::new(0xA4, 0x0E, 0x26),
                Rgb::new(0x1A, 0x7F, 0x37),
                Rgb::new(0x63, 0x3C, 0x01),
                Rgb::new(0x21, 0x8B, 0xFF),
                Rgb::new(0xA4, 0x75, 0xF9),
                Rgb::new(0x31, 0x92, 0xAA),
                Rgb::new(0x8C, 0x95, 0x9F),
            ],
        }
    }

    pub const fn catppuccin_mocha() -> Self {
        Self {
            background: Rgb::new(0x1E, 0x1E, 0x2E),
            foreground: Rgb::new(0xCD, 0xD6, 0xF4),
            selection: Rgb::new(0x45, 0x47, 0x5A),
            ansi: [
                Rgb::new(0x45, 0x47, 0x5A),
                Rgb::new(0xF3, 0x8B, 0xA8),
                Rgb::new(0xA6, 0xE3, 0xA1),
                Rgb::new(0xF9, 0xE2, 0xAF),
                Rgb::new(0x89, 0xB4, 0xFA),
                Rgb::new(0xF5, 0xC2, 0xE7),
                Rgb::new(0x94, 0xE2, 0xD5),
                Rgb::new(0xBA, 0xC2, 0xDE),
                Rgb::new(0x58, 0x5B, 0x70),
                Rgb::new(0xF3, 0x8B, 0xA8),
                Rgb::new(0xA6, 0xE3, 0xA1),
                Rgb::new(0xF9, 0xE2, 0xAF),
                Rgb::new(0x89, 0xB4, 0xFA),
                Rgb::new(0xF5, 0xC2, 0xE7),
                Rgb::new(0x94, 0xE2, 0xD5),
                Rgb::new(0xA6, 0xAD, 0xC8),
            ],
        }
    }

    pub const fn catppuccin_latte() -> Self {
        Self {
            background: Rgb::new(0xEF, 0xF1, 0xF5),
            foreground: Rgb::new(0x4C, 0x4F, 0x69),
            selection: Rgb::new(0xBC, 0xC0, 0xCC),
            ansi: [
                Rgb::new(0xBC, 0xC0, 0xCC),
                Rgb::new(0xD2, 0x0F, 0x39),
                Rgb::new(0x40, 0xA0, 0x2B),
                Rgb::new(0xDF, 0x8E, 0x1D),
                Rgb::new(0x1E, 0x66, 0xF5),
                Rgb::new(0xEA, 0x76, 0xCB),
                Rgb::new(0x17, 0x92, 0x99),
                Rgb::new(0x5C, 0x5F, 0x77),
                Rgb::new(0xAC, 0xB0, 0xBE),
                Rgb::new(0xD2, 0x0F, 0x39),
                Rgb::new(0x40, 0xA0, 0x2B),
                Rgb::new(0xDF, 0x8E, 0x1D),
                Rgb::new(0x1E, 0x66, 0xF5),
                Rgb::new(0xEA, 0x76, 0xCB),
                Rgb::new(0x17, 0x92, 0x99),
                Rgb::new(0x6C, 0x6F, 0x85),
            ],
        }
    }

    pub const fn one_dark() -> Self {
        Self {
            background: Rgb::new(0x28, 0x2C, 0x34),
            foreground: Rgb::new(0xDC, 0xDF, 0xE4),
            selection: Rgb::new(0x47, 0x4E, 0x5D),
            ansi: [
                Rgb::new(0x3F, 0x44, 0x51),
                Rgb::new(0xE0, 0x6C, 0x75),
                Rgb::new(0x98, 0xC3, 0x79),
                Rgb::new(0xE5, 0xC0, 0x7B),
                Rgb::new(0x61, 0xAF, 0xEF),
                Rgb::new(0xC6, 0x78, 0xDD),
                Rgb::new(0x56, 0xB6, 0xC2),
                Rgb::new(0xDC, 0xDF, 0xE4),
                Rgb::new(0x5C, 0x63, 0x70),
                Rgb::new(0xE0, 0x6C, 0x75),
                Rgb::new(0x98, 0xC3, 0x79),
                Rgb::new(0xE5, 0xC0, 0x7B),
                Rgb::new(0x61, 0xAF, 0xEF),
                Rgb::new(0xC6, 0x78, 0xDD),
                Rgb::new(0x56, 0xB6, 0xC2),
                Rgb::new(0xFF, 0xFF, 0xFF),
            ],
        }
    }

    pub const fn one_light() -> Self {
        Self {
            background: Rgb::new(0xFA, 0xFA, 0xFA),
            foreground: Rgb::new(0x38, 0x3A, 0x42),
            selection: Rgb::new(0xBF, 0xCE, 0xFF),
            ansi: [
                Rgb::new(0x38, 0x3A, 0x42),
                Rgb::new(0xE4, 0x56, 0x49),
                Rgb::new(0x50, 0xA1, 0x4F),
                Rgb::new(0xC1, 0x84, 0x01),
                Rgb::new(0x01, 0x84, 0xBC),
                Rgb::new(0xA6, 0x26, 0xA4),
                Rgb::new(0x09, 0x97, 0xB3),
                Rgb::new(0xFA, 0xFA, 0xFA),
                Rgb::new(0x4F, 0x52, 0x5E),
                Rgb::new(0xE4, 0x56, 0x49),
                Rgb::new(0x50, 0xA1, 0x4F),
                Rgb::new(0xC1, 0x84, 0x01),
                Rgb::new(0x01, 0x84, 0xBC),
                Rgb::new(0xA6, 0x26, 0xA4),
                Rgb::new(0x09, 0x97, 0xB3),
                Rgb::new(0xFF, 0xFF, 0xFF),
            ],
        }
    }

    pub const fn tokyo_night() -> Self {
        Self {
            background: Rgb::new(0x1A, 0x1B, 0x26),
            foreground: Rgb::new(0xC0, 0xCA, 0xF5),
            selection: Rgb::new(0x28, 0x34, 0x57),
            ansi: [
                Rgb::new(0x15, 0x16, 0x1E),
                Rgb::new(0xF7, 0x76, 0x8E),
                Rgb::new(0x9E, 0xCE, 0x6A),
                Rgb::new(0xE0, 0xAF, 0x68),
                Rgb::new(0x7A, 0xA2, 0xF7),
                Rgb::new(0xBB, 0x9A, 0xF7),
                Rgb::new(0x7D, 0xCF, 0xFF),
                Rgb::new(0xA9, 0xB1, 0xD6),
                Rgb::new(0x41, 0x48, 0x68),
                Rgb::new(0xF7, 0x76, 0x8E),
                Rgb::new(0x9E, 0xCE, 0x6A),
                Rgb::new(0xE0, 0xAF, 0x68),
                Rgb::new(0x7A, 0xA2, 0xF7),
                Rgb::new(0xBB, 0x9A, 0xF7),
                Rgb::new(0x7D, 0xCF, 0xFF),
                Rgb::new(0xC0, 0xCA, 0xF5),
            ],
        }
    }

    pub const fn tokyo_day() -> Self {
        Self {
            background: Rgb::new(0xE1, 0xE2, 0xE7),
            foreground: Rgb::new(0x37, 0x60, 0xBF),
            selection: Rgb::new(0xB7, 0xC1, 0xE3),
            ansi: [
                Rgb::new(0xE9, 0xE9, 0xED),
                Rgb::new(0xF5, 0x2A, 0x65),
                Rgb::new(0x58, 0x75, 0x39),
                Rgb::new(0x8C, 0x6C, 0x3E),
                Rgb::new(0x2E, 0x7D, 0xE9),
                Rgb::new(0x98, 0x54, 0xF1),
                Rgb::new(0x00, 0x71, 0x97),
                Rgb::new(0x61, 0x72, 0xB0),
                Rgb::new(0xA1, 0xA6, 0xC5),
                Rgb::new(0xF5, 0x2A, 0x65),
                Rgb::new(0x58, 0x75, 0x39),
                Rgb::new(0x8C, 0x6C, 0x3E),
                Rgb::new(0x2E, 0x7D, 0xE9),
                Rgb::new(0x98, 0x54, 0xF1),
                Rgb::new(0x00, 0x71, 0x97),
                Rgb::new(0x37, 0x60, 0xBF),
            ],
        }
    }

    pub const fn variant(theme: TermTheme) -> Self {
        match theme {
            TermTheme::Dark => Self::dark(),
            TermTheme::Light => Self::light(),
        }
    }

    pub const fn ansi(&self, index: u8) -> Rgb {
        self.ansi[index as usize & 0x0F]
    }

    pub fn resolve(&self, color: Color) -> Rgb {
        match color {
            Color::Named(named) => self.named(named),
            Color::Indexed(index) => self.indexed(index),
            Color::Spec(rgb) => Rgb::new(rgb.r, rgb.g, rgb.b),
        }
    }

    pub fn query_color(&self, index: usize) -> Option<Rgb> {
        match index {
            0..=255 => Some(self.indexed(index as u8)),
            index if index == NamedColor::Foreground as usize => Some(self.foreground),
            index if index == NamedColor::Background as usize => Some(self.background),
            index if index == NamedColor::Cursor as usize => Some(self.foreground),
            index if index == NamedColor::DimBlack as usize => Some(self.dim(self.ansi(0))),
            index if index == NamedColor::DimRed as usize => Some(self.dim(self.ansi(1))),
            index if index == NamedColor::DimGreen as usize => Some(self.dim(self.ansi(2))),
            index if index == NamedColor::DimYellow as usize => Some(self.dim(self.ansi(3))),
            index if index == NamedColor::DimBlue as usize => Some(self.dim(self.ansi(4))),
            index if index == NamedColor::DimMagenta as usize => Some(self.dim(self.ansi(5))),
            index if index == NamedColor::DimCyan as usize => Some(self.dim(self.ansi(6))),
            index if index == NamedColor::DimWhite as usize => Some(self.dim(self.ansi(7))),
            index if index == NamedColor::BrightForeground as usize => Some(self.foreground),
            index if index == NamedColor::DimForeground as usize => Some(self.dim(self.foreground)),
            _ => None,
        }
    }

    pub fn dim(&self, color: Rgb) -> Rgb {
        Rgb::new(
            dim_channel(color.r, self.background.r),
            dim_channel(color.g, self.background.g),
            dim_channel(color.b, self.background.b),
        )
    }

    fn named(&self, named: NamedColor) -> Rgb {
        match named {
            NamedColor::Black
            | NamedColor::Red
            | NamedColor::Green
            | NamedColor::Yellow
            | NamedColor::Blue
            | NamedColor::Magenta
            | NamedColor::Cyan
            | NamedColor::White
            | NamedColor::BrightBlack
            | NamedColor::BrightRed
            | NamedColor::BrightGreen
            | NamedColor::BrightYellow
            | NamedColor::BrightBlue
            | NamedColor::BrightMagenta
            | NamedColor::BrightCyan
            | NamedColor::BrightWhite => self.ansi(named as u8),
            NamedColor::Background => self.background,
            NamedColor::Foreground | NamedColor::BrightForeground | NamedColor::Cursor => {
                self.foreground
            }
            NamedColor::DimForeground => self.dim(self.foreground),
            NamedColor::DimBlack => self.dim(self.ansi(0)),
            NamedColor::DimRed => self.dim(self.ansi(1)),
            NamedColor::DimGreen => self.dim(self.ansi(2)),
            NamedColor::DimYellow => self.dim(self.ansi(3)),
            NamedColor::DimBlue => self.dim(self.ansi(4)),
            NamedColor::DimMagenta => self.dim(self.ansi(5)),
            NamedColor::DimCyan => self.dim(self.ansi(6)),
            NamedColor::DimWhite => self.dim(self.ansi(7)),
        }
    }

    fn indexed(&self, index: u8) -> Rgb {
        match index {
            0..=15 => self.ansi(index),
            16..=231 => cube_color(index),
            232..=255 => grayscale_color(index),
        }
    }
}

const CUBE_STEPS: [u8; 6] = [0x00, 0x5F, 0x87, 0xAF, 0xD7, 0xFF];

fn cube_color(index: u8) -> Rgb {
    let i = index - 16;
    Rgb::new(
        CUBE_STEPS[(i / 36) as usize],
        CUBE_STEPS[((i / 6) % 6) as usize],
        CUBE_STEPS[(i % 6) as usize],
    )
}

fn grayscale_color(index: u8) -> Rgb {
    let level = 8 + (index - 232) * 10;
    Rgb::new(level, level, level)
}

fn dim_channel(ink: u8, bg: u8) -> u8 {
    ((ink as u16 * DIM_INK_PERCENT + bg as u16 * DIM_BG_PERCENT) / 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::ansi::Rgb as VteRgb;

    fn hex(rgb: Rgb) -> u32 {
        ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
    }

    #[test]
    fn dark_ansi_table_matches_terminal_spec() {
        let p = TermPalette::dark();
        let expected = [
            0x232D3B, 0xE5484D, 0x46A758, 0xE2A03F, 0x4F86E8, 0xB05CCC, 0x38A3A5, 0xC9CACC,
            0x5A6374, 0xFF6369, 0x5DC971, 0xF2C55C, 0x6E9CEC, 0xC77DDB, 0x4FC3C5, 0xECECEC,
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(hex(p.ansi(i as u8)), *want, "dark ansi index {i}");
        }
        assert_eq!(hex(p.background), 0x19222D);
        assert_eq!(hex(p.foreground), 0xECECEC);
    }

    #[test]
    fn light_ansi_table_matches_terminal_spec() {
        let p = TermPalette::light();
        let expected = [
            0x1E2030, 0xC42B2B, 0x297A3A, 0x9A6700, 0x2E68D3, 0x8E44AD, 0x1F7A8C, 0xD2D4D9,
            0x6A6B6E, 0xE5484D, 0x46A758, 0xB7791F, 0x4579D8, 0xA65BC2, 0x2A93A6, 0xFFFFFF,
        ];
        for (i, want) in expected.iter().enumerate() {
            assert_eq!(hex(p.ansi(i as u8)), *want, "light ansi index {i}");
        }
        assert_eq!(hex(p.background), 0xFFFFFF);
        assert_eq!(hex(p.foreground), 0x1E2030);
    }

    #[test]
    fn named_colors_map_to_ansi_and_special_slots() {
        let p = TermPalette::dark();
        assert_eq!(p.resolve(Color::Named(NamedColor::Black)), p.ansi(0));
        assert_eq!(p.resolve(Color::Named(NamedColor::Red)), p.ansi(1));
        assert_eq!(p.resolve(Color::Named(NamedColor::BrightWhite)), p.ansi(15));
        assert_eq!(
            p.resolve(Color::Named(NamedColor::Foreground)),
            p.foreground
        );
        assert_eq!(
            p.resolve(Color::Named(NamedColor::Background)),
            p.background
        );
    }

    #[test]
    fn query_colors_resolve_terminal_dynamic_indices() {
        let p = TermPalette::dark();
        assert_eq!(p.query_color(0), Some(p.ansi(0)));
        assert_eq!(p.query_color(196), Some(Rgb::new(0xFF, 0, 0)));
        assert_eq!(
            p.query_color(NamedColor::Foreground as usize),
            Some(p.foreground)
        );
        assert_eq!(
            p.query_color(NamedColor::Background as usize),
            Some(p.background)
        );
        assert_eq!(
            p.query_color(NamedColor::DimForeground as usize),
            Some(p.dim(p.foreground))
        );
    }

    #[test]
    fn dim_named_colors_are_resolved_toward_the_background() {
        let p = TermPalette::dark();
        assert_eq!(
            p.resolve(Color::Named(NamedColor::DimForeground)),
            p.dim(p.foreground)
        );
        assert_ne!(
            p.resolve(Color::Named(NamedColor::DimForeground)),
            p.foreground,
            "faint default foreground must not render as bright foreground"
        );
        assert_eq!(
            p.resolve(Color::Named(NamedColor::DimBlack)),
            p.dim(p.ansi(0)),
            "DimBlack is a real named color, not an integer-wrapped ANSI index"
        );
    }

    #[test]
    fn indexed_low_uses_ansi_table() {
        let p = TermPalette::dark();
        for i in 0u8..16 {
            assert_eq!(p.resolve(Color::Indexed(i)), p.ansi(i), "indexed {i}");
        }
    }

    #[test]
    fn indexed_cube_and_grayscale_are_variant_independent() {
        let dark = TermPalette::dark();
        let light = TermPalette::light();
        for i in 16u8..=255 {
            assert_eq!(
                dark.resolve(Color::Indexed(i)),
                light.resolve(Color::Indexed(i)),
                "indexed {i} must be theme-independent",
            );
        }
    }

    #[test]
    fn indexed_cube_endpoints_match_xterm_formula() {
        let p = TermPalette::dark();
        assert_eq!(p.resolve(Color::Indexed(16)), Rgb::new(0, 0, 0));
        assert_eq!(p.resolve(Color::Indexed(231)), Rgb::new(0xFF, 0xFF, 0xFF));
        assert_eq!(p.resolve(Color::Indexed(196)), Rgb::new(0xFF, 0, 0));
        assert_eq!(p.resolve(Color::Indexed(46)), Rgb::new(0, 0xFF, 0));
        assert_eq!(p.resolve(Color::Indexed(21)), Rgb::new(0, 0, 0xFF));
    }

    #[test]
    fn indexed_grayscale_ramp_matches_xterm_formula() {
        let p = TermPalette::dark();
        assert_eq!(p.resolve(Color::Indexed(232)), Rgb::new(8, 8, 8));
        assert_eq!(p.resolve(Color::Indexed(255)), Rgb::new(238, 238, 238));
    }

    #[test]
    fn true_color_spec_passes_through_unchanged() {
        let dark = TermPalette::dark();
        let light = TermPalette::light();
        let spec = Color::Spec(VteRgb {
            r: 0x12,
            g: 0x34,
            b: 0x56,
        });
        assert_eq!(dark.resolve(spec), Rgb::new(0x12, 0x34, 0x56));
        assert_eq!(light.resolve(spec), dark.resolve(spec));
    }

    #[test]
    fn variant_selects_table_independent_of_chrome_mode() {
        assert_eq!(TermPalette::variant(TermTheme::Dark), TermPalette::dark());
        assert_eq!(TermPalette::variant(TermTheme::Light), TermPalette::light());
        assert_ne!(TermPalette::dark(), TermPalette::light());
    }

    #[test]
    fn named_palettes_are_distinct_and_keep_foreground_off_background() {
        let palettes = [
            TermPalette::dark(),
            TermPalette::light(),
            TermPalette::github_dark(),
            TermPalette::github_light(),
            TermPalette::catppuccin_mocha(),
            TermPalette::catppuccin_latte(),
            TermPalette::one_dark(),
            TermPalette::one_light(),
            TermPalette::tokyo_night(),
            TermPalette::tokyo_day(),
        ];
        for (i, a) in palettes.iter().enumerate() {
            assert_ne!(a.background, a.foreground, "palette {i} fg == bg");
            for b in palettes.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn catppuccin_tables_match_the_official_mocha_and_latte_ansi() {
        let mocha = TermPalette::catppuccin_mocha();
        let latte = TermPalette::catppuccin_latte();
        assert_eq!(hex(mocha.background), 0x1E1E2E);
        assert_eq!(hex(mocha.ansi(4)), 0x89B4FA, "mocha blue");
        assert_eq!(hex(latte.background), 0xEFF1F5);
        assert_eq!(hex(latte.ansi(4)), 0x1E66F5, "latte blue");
    }
}
