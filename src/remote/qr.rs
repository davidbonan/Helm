//! The pairing URL as QR modules (specs/remote.md §2), for the modal to paint.

use qrcode::{Color, QrCode};

pub struct QrMatrix {
    width: usize,
    dark: Vec<bool>,
}

impl QrMatrix {
    pub fn encode(text: &str) -> Option<Self> {
        let code = QrCode::new(text.as_bytes()).ok()?;
        Some(Self {
            width: code.width(),
            dark: code
                .to_colors()
                .into_iter()
                .map(|c| c == Color::Dark)
                .collect(),
        })
    }

    /// Modules per side, quiet zone excluded.
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn is_dark(&self, x: usize, y: usize) -> bool {
        self.dark[y * self.width + x]
    }
}
