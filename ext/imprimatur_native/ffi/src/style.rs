//! `Imprimatur::Style`: an immutable, validated cell format, built once and
//! shared by every cell that uses it.

use imprimatur::xlsx::{Border, Color, Format, VerticalAlign};
use magnus::{Error, Ruby};

use crate::{ALIGNS, DIRECTIONS, OrRaise, keyword};

#[magnus::wrap(class = "Imprimatur::Style", free_immediately, size)]
pub struct Style(pub Format);

impl Style {
    /// Called by `Imprimatur.style`, which owns the keyword interface.
    #[expect(clippy::too_many_arguments, reason = "one positional argument per Ruby keyword")]
    #[expect(clippy::fn_params_excessive_bools, reason = "they arrive as Ruby keywords")]
    pub fn build(
        ruby: &Ruby,
        number_format: Option<String>,
        bold: bool,
        italic: bool,
        underline: bool,
        color: Option<String>,
        fill: Option<String>,
        align: Option<String>,
        direction: Option<String>,
        wrap: bool,
        border: Option<String>,
        font: Option<String>,
        size: Option<f64>,
        valign: Option<String>,
    ) -> Result<Self, Error> {
        let mut format = Format::new();
        if let Some(code) = number_format {
            format = format.number_format(code).or_raise(ruby)?;
        }
        if let Some(font) = font {
            format = format.font_name(font).or_raise(ruby)?;
        }
        if let Some(size) = size {
            format = format.font_size(size).or_raise(ruby)?;
        }
        if let Some(valign) = valign {
            let choices = [
                ("top", VerticalAlign::Top),
                ("center", VerticalAlign::Center),
                ("bottom", VerticalAlign::Bottom),
            ];
            format = format.valign(keyword(ruby, "valign", &valign, &choices)?);
        }
        if bold {
            format = format.bold();
        }
        if italic {
            format = format.italic();
        }
        if underline {
            format = format.underline();
        }
        if wrap {
            format = format.wrap();
        }
        if let Some(color) = color {
            format = format.font_color(parse_color(ruby, &color)?);
        }
        if let Some(fill) = fill {
            format = format.fill(parse_color(ruby, &fill)?);
        }
        if let Some(align) = align {
            format = format.align(keyword(ruby, "align", &align, &ALIGNS)?);
        }
        if let Some(direction) = direction {
            format = format.direction(keyword(ruby, "direction", &direction, &DIRECTIONS)?);
        }
        if let Some(border) = border {
            let choices =
                [("thin", Border::Thin), ("medium", Border::Medium), ("thick", Border::Thick)];
            format = format.border(keyword(ruby, "border", &border, &choices)?);
        }
        Ok(Self(format))
    }
}

fn parse_color(ruby: &Ruby, hex: &str) -> Result<Color, Error> {
    Color::from_hex(hex).or_raise(ruby)
}
