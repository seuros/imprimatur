//! How a cell looks: number format, font, fill, alignment and border.

#[cfg(test)]
mod tests;

use crate::style::{Align, Color, TextDirection};
use crate::{Error, Result};

/// Excel's limit on a number format code.
const MAX_NUMBER_FORMAT: usize = 255;
/// Excel's limit on a font name.
const MAX_FONT_NAME: usize = 31;
/// The largest font size Excel accepts, in points.
const MAX_FONT_SIZE: f64 = 409.0;
/// The font of unformatted cells.
pub const DEFAULT_FONT: &str = "Calibri";
/// Its size, in half points.
const DEFAULT_HALF_POINTS: u16 = 22;

/// Vertical alignment within the cell. Excel's default is the bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VerticalAlign {
    Top,
    Center,
    Bottom,
}

impl VerticalAlign {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Center => "center",
            Self::Bottom => "bottom",
        }
    }
}

/// A line on all four sides of the cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Border {
    Thin,
    Medium,
    Thick,
}

impl Border {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Thin => "thin",
            Self::Medium => "medium",
            Self::Thick => "thick",
        }
    }
}

/// A cell format, built up from the default look:
///
/// ```
/// use imprimatur::xlsx::{Align, Color, Format};
///
/// let money = Format::new().number_format("#,##0.00")?.bold().fill(Color::rgb(0xFFF2CC));
/// let header = Format::new().bold().align(Align::Center);
/// # Ok::<(), imprimatur::Error>(())
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Format {
    pub(crate) number_code: Option<String>,
    pub(crate) font: Font,
    pub(crate) fill: Option<Color>,
    pub(crate) align: Option<Align>,
    pub(crate) valign: Option<VerticalAlign>,
    pub(crate) direction: Option<TextDirection>,
    pub(crate) wrap: bool,
    pub(crate) border: Option<Border>,
}

impl Format {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An Excel number format code, such as `0.00`, `#,##0`, `0%` or
    /// `yyyy-mm-dd`. `General` is the default.
    ///
    /// # Errors
    /// [`Error::InvalidNumberFormat`] when empty, over 255 characters, or
    /// holding a control character.
    pub fn number_format(mut self, code: impl Into<String>) -> Result<Self> {
        let code = code.into();
        let length = code.chars().count();
        if length == 0 || length > MAX_NUMBER_FORMAT || code.chars().any(char::is_control) {
            return Err(Error::InvalidNumberFormat(code));
        }
        self.number_code = Some(code);
        Ok(self)
    }

    #[must_use]
    pub const fn bold(mut self) -> Self {
        self.font.bold = true;
        self
    }

    #[must_use]
    pub const fn italic(mut self) -> Self {
        self.font.italic = true;
        self
    }

    #[must_use]
    pub const fn underline(mut self) -> Self {
        self.font.underline = true;
        self
    }

    #[must_use]
    pub const fn font_color(mut self, color: Color) -> Self {
        self.font.color = Some(color);
        self
    }

    /// The typeface, such as `Arial`; Calibri by default. Excel substitutes
    /// another font when the reader's machine lacks it.
    ///
    /// # Errors
    /// [`Error::InvalidFontName`] when empty, over 31 characters, or holding
    /// a control character.
    pub fn font_name(mut self, name: impl Into<String>) -> Result<Self> {
        let name = name.into();
        let length = name.chars().count();
        if length == 0 || length > MAX_FONT_NAME || name.chars().any(char::is_control) {
            return Err(Error::InvalidFontName(name));
        }
        self.font.name = (name != DEFAULT_FONT).then_some(name);
        Ok(self)
    }

    /// The size in points, 11 by default.
    ///
    /// # Errors
    /// [`Error::InvalidFontSize`] outside 1 to 409, or between half points.
    pub fn font_size(mut self, points: f64) -> Result<Self> {
        let half_points = points * 2.0;
        let valid = (1.0..=MAX_FONT_SIZE).contains(&points) && half_points.fract() == 0.0;
        valid.ok_or(Error::InvalidFontSize(points))?;
        #[expect(clippy::cast_possible_truncation, reason = "a whole number from 2 to 818")]
        #[expect(clippy::cast_sign_loss, reason = "a whole number from 2 to 818")]
        let half_points = half_points as u16;
        self.font.half_points = (half_points != DEFAULT_HALF_POINTS).then_some(half_points);
        Ok(self)
    }

    /// A solid background.
    #[must_use]
    pub const fn fill(mut self, color: Color) -> Self {
        self.fill = Some(color);
        self
    }

    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    #[must_use]
    pub const fn valign(mut self, valign: VerticalAlign) -> Self {
        self.valign = Some(valign);
        self
    }

    #[must_use]
    pub const fn direction(mut self, direction: TextDirection) -> Self {
        self.direction = Some(direction);
        self
    }

    /// Wraps text onto several lines within the column width.
    #[must_use]
    pub const fn wrap(mut self) -> Self {
        self.wrap = true;
        self
    }

    #[must_use]
    pub const fn border(mut self, border: Border) -> Self {
        self.border = Some(border);
        self
    }

    pub(crate) const fn has_number_format(&self) -> bool {
        self.number_code.is_some()
    }

    /// Whether the format needs an `<alignment>` element.
    pub(crate) const fn has_alignment(&self) -> bool {
        self.align.is_some() || self.valign.is_some() || self.direction.is_some() || self.wrap
    }
}

/// The part of a format that lives in `<fonts>`. The default typeface and
/// size are `None`, so a format naming them equals one that does not.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Font {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) color: Option<Color>,
    pub(crate) name: Option<String>,
    /// Points times two: sizes come in steps of 0.5.
    pub(crate) half_points: Option<u16>,
}

impl Font {
    pub(crate) fn name(&self) -> &str {
        self.name.as_deref().unwrap_or(DEFAULT_FONT)
    }

    /// The size in points as `<sz>` writes it: `11`, `10.5`.
    pub(crate) fn size(&self) -> String {
        let half_points = self.half_points.unwrap_or(DEFAULT_HALF_POINTS);
        let whole = half_points / 2;
        if half_points.is_multiple_of(2) { whole.to_string() } else { format!("{whole}.5") }
    }
}
