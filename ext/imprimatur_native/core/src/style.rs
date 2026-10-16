//! What every format shares: colors, alignment and text direction.

#[cfg(test)]
mod tests;

use crate::{Error, Result};

/// An opaque RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color(u32);

impl Color {
    pub const BLACK: Self = Self(0x00_0000);
    pub const WHITE: Self = Self(0xFF_FFFF);

    /// A color from `0xRRGGBB`; bits above 24 are ignored.
    #[must_use]
    pub const fn rgb(rgb: u32) -> Self {
        Self(rgb & 0xFF_FFFF)
    }

    /// Parses `#RRGGBB` or `RRGGBB`.
    ///
    /// # Errors
    /// [`Error::InvalidColor`] for anything else.
    pub fn from_hex(hex: &str) -> Result<Self> {
        let digits = hex.strip_prefix('#').unwrap_or(hex);
        (digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .ok_or_else(|| Error::InvalidColor(hex.to_owned()))?;
        u32::from_str_radix(digits, 16).map(Self).map_err(|_| Error::InvalidColor(hex.to_owned()))
    }

    /// Red, green and blue from 0 to 1, as PDF color operators take them.
    pub(crate) fn components(self) -> [f64; 3] {
        let [_, r, g, b] = self.0.to_be_bytes();
        [r, g, b].map(|channel| f64::from(channel) / 255.0)
    }

    /// `FFRRGGBB`, the opaque ARGB `SpreadsheetML` writes.
    pub(crate) fn argb(self) -> String {
        format!("FF{:06X}", self.0)
    }
}

/// Horizontal alignment within the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    Left,
    Center,
    Right,
}

impl Align {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// The direction text reads in.
///
/// For the right-to-left scripts: Hebrew (and Yiddish), Arabic (and Persian,
/// Urdu, Pashto, Kurdish), Syriac, Thaana, N'Ko, Adlam and the rest. Without
/// one, Excel infers it from the first strong character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextDirection {
    LeftToRight,
    RightToLeft,
}

impl TextDirection {
    /// `readingOrder`: 1 left to right, 2 right to left (0 is context).
    pub(crate) const fn reading_order(self) -> &'static str {
        match self {
            Self::LeftToRight => "1",
            Self::RightToLeft => "2",
        }
    }
}
