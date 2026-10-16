//! The four Helvetica faces: `WinAnsiEncoding` and advance widths.

#[cfg(test)]
mod tests;

use super::metrics::{
    FontMetrics, HELVETICA, HELVETICA_BOLD, HELVETICA_BOLDOBLIQUE, HELVETICA_OBLIQUE, WIN_ANSI_HIGH,
};
use crate::{Error, Result};

/// A face of the body font; its index is its `/F1`..`/F4` resource number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl Face {
    pub const ALL: [Self; 4] = [Self::Regular, Self::Bold, Self::Italic, Self::BoldItalic];

    pub const fn new(bold: bool, italic: bool) -> Self {
        match (bold, italic) {
            (false, false) => Self::Regular,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }

    pub const fn metrics(self) -> &'static FontMetrics {
        match self {
            Self::Regular => &HELVETICA,
            Self::Bold => &HELVETICA_BOLD,
            Self::Italic => &HELVETICA_OBLIQUE,
            Self::BoldItalic => &HELVETICA_BOLDOBLIQUE,
        }
    }

    /// The resource name: `F1` to `F4`.
    pub const fn resource(self) -> &'static str {
        match self {
            Self::Regular => "F1",
            Self::Bold => "F2",
            Self::Italic => "F3",
            Self::BoldItalic => "F4",
        }
    }

    /// The text in `WinAnsiEncoding`, the encoding the standard fonts use here.
    ///
    /// # Errors
    /// [`Error::UnsupportedCharacter`] for anything Windows-1252 has no byte for
    /// or the font has no glyph for.
    pub fn encode(self, text: &str) -> Result<Vec<u8>> {
        let metrics = self.metrics();
        text.chars()
            .map(|ch| {
                win_ansi(ch)
                    .filter(|&byte| {
                        ch == '\n' || ch == '\t' || metrics.widths[usize::from(byte)] > 0
                    })
                    .ok_or(Error::UnsupportedCharacter { character: ch, font: metrics.name })
            })
            .collect()
    }

    /// The advance width of encoded text at `size` points.
    pub fn width(self, bytes: &[u8], size: f64) -> f64 {
        let widths = &self.metrics().widths;
        let units: u32 = bytes.iter().map(|&byte| u32::from(widths[usize::from(byte)])).sum();
        f64::from(units) * size / 1000.0
    }

    /// The height above the baseline at `size` points.
    pub fn ascent(self, size: f64) -> f64 {
        f64::from(self.metrics().ascender) * size / 1000.0
    }

    /// The depth below the baseline at `size` points, as a positive number.
    pub fn descent(self, size: f64) -> f64 {
        -f64::from(self.metrics().descender) * size / 1000.0
    }
}

/// The Windows-1252 byte for a character: Latin-1 maps to itself, except
/// 0x80..=0x9F, which Windows-1252 gives typographic characters.
fn win_ansi(ch: char) -> Option<u8> {
    let code = u32::from(ch);
    if code < 0x80 || (0xA0..=0xFF).contains(&code) {
        return u8::try_from(code).ok();
    }
    let index = WIN_ANSI_HIGH.iter().position(|&high| high == Some(ch))?;
    u8::try_from(0x80 + index).ok()
}
