//! Pictures: PNG or JPEG files, read once for what PDF and DOCX need.

use std::sync::Arc;

use super::size;
use crate::style::Align;
use crate::{Error, Result};

/// Pixels per point at 96 pixels an inch: a picture's size when none is set.
const POINTS_PER_PIXEL: f64 = 0.75;
/// The largest side, in pixels, a picture may have.
const MAX_PIXELS: u32 = 1 << 16;

/// A PNG or JPEG drawn in the document's flow.
///
/// It is drawn at the width or height set (in points, the other following
/// its proportions), or else at 96 pixels an inch, never larger than the
/// page leaves room for.
///
/// ```no_run
/// use imprimatur::doc::{Document, Picture};
/// use imprimatur::style::Align;
///
/// let mut doc = Document::new();
/// doc.picture(Picture::new(std::fs::read("logo.png")?)?.width(144.0)?.align(Align::Center));
/// # Ok::<(), imprimatur::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub(crate) data: Arc<[u8]>,
    pub(crate) kind: Kind,
    pub(crate) pixels: (u32, u32),
    width: Option<f64>,
    height: Option<f64>,
    pub(crate) align: Option<Align>,
}

/// The format, and what writing it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Png(Png),
    /// Gray (1), RGB (3) or CMYK (4) components; Adobe's CMYK is inverted.
    Jpeg {
        components: u8,
        inverted: bool,
    },
}

/// A PNG's pixel layout and its compressed pixels (its `IDAT` chunks).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Png {
    pub depth: u8,
    /// 0 gray, 2 RGB, 3 palette, 4 gray and alpha, 6 RGB and alpha.
    pub color: u8,
    pub palette: Vec<u8>,
    /// The `tRNS` chunk: a palette's alphas, or the transparent color.
    pub transparency: Vec<u8>,
    pub compressed: Vec<u8>,
}

impl Picture {
    /// # Errors
    /// [`Error::InvalidImage`] for data that is no PNG or JPEG, a damaged
    /// one, an interlaced PNG, a JPEG other than 8 bits a sample, or one
    /// larger than 65 536 pixels a side.
    pub fn new(data: impl Into<Vec<u8>>) -> Result<Self> {
        let data: Arc<[u8]> = data.into().into();
        let (kind, pixels) = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            png(&data)?
        } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
            jpeg(&data)?
        } else {
            return Err(Error::InvalidImage("not a PNG or JPEG image"));
        };
        let valid = (1..=MAX_PIXELS).contains(&pixels.0) && (1..=MAX_PIXELS).contains(&pixels.1);
        valid.ok_or(Error::InvalidImage("a picture of 1 to 65 536 pixels a side"))?;
        Ok(Self { data, kind, pixels, width: None, height: None, align: None })
    }

    /// Its width in points; the height follows unless set too.
    ///
    /// # Errors
    /// [`Error::InvalidImage`] outside 1 to 14 400 points.
    pub fn width(mut self, points: f64) -> Result<Self> {
        self.width = Some(points_in_range(points)?);
        Ok(self)
    }

    /// Its height in points; the width follows unless set too.
    ///
    /// # Errors
    /// [`Error::InvalidImage`] outside 1 to 14 400 points.
    pub fn height(mut self, points: f64) -> Result<Self> {
        self.height = Some(points_in_range(points)?);
        Ok(self)
    }

    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// The size it is drawn at, in points, fitted into `room` (the width
    /// and height between the margins).
    pub(crate) fn size(&self, room: (f64, f64)) -> (f64, f64) {
        let (pixels_wide, pixels_high) = (f64::from(self.pixels.0), f64::from(self.pixels.1));
        let (width, height) = match (self.width, self.height) {
            (Some(width), Some(height)) => (width, height),
            (Some(width), None) => (width, width * pixels_high / pixels_wide),
            (None, Some(height)) => (height * pixels_wide / pixels_high, height),
            (None, None) => (pixels_wide * POINTS_PER_PIXEL, pixels_high * POINTS_PER_PIXEL),
        };
        size::fit((width, height), room)
    }
}

fn points_in_range(points: f64) -> Result<f64> {
    size::points_in_range(points)
        .ok_or(Error::InvalidImage("a picture of 1 to 14 400 points a side"))
}

/// A PNG's chunks; interlaced ones are refused.
fn png(data: &[u8]) -> Result<(Kind, (u32, u32))> {
    let damaged = || Error::InvalidImage("a damaged PNG");
    let mut png = Png {
        depth: 0,
        color: 0,
        palette: Vec::new(),
        transparency: Vec::new(),
        compressed: Vec::new(),
    };
    let (mut pixels, mut at) = (None, 8);
    while let Some(head) = data.get(at..at + 8) {
        let length = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
        let body = data.get(at + 8..at + 8 + length).ok_or_else(damaged)?;
        match &head[4..8] {
            b"IHDR" if body.len() == 13 => {
                let side = |at: usize| {
                    u32::from_be_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]])
                };
                pixels = Some((side(0), side(4)));
                (png.depth, png.color) = (body[8], body[9]);
                (body[12] == 0).ok_or(Error::InvalidImage("an interlaced PNG"))?;
            }
            b"PLTE" => png.palette = body.to_vec(),
            b"tRNS" => png.transparency = body.to_vec(),
            b"IDAT" => png.compressed.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        at += 12 + length;
    }
    let layout = matches!(
        (png.color, png.depth),
        (0, 1 | 2 | 4 | 8 | 16) | (3, 1 | 2 | 4 | 8) | (2 | 4 | 6, 8 | 16)
    );
    let palette = png.color != 3 || !png.palette.is_empty();
    let pixels = pixels.filter(|_| layout && palette && !png.compressed.is_empty());
    Ok((Kind::Png(png), pixels.ok_or_else(damaged)?))
}

/// A JPEG's frame header (`SOF`), and Adobe's mark, which inverts CMYK.
fn jpeg(data: &[u8]) -> Result<(Kind, (u32, u32))> {
    let damaged = || Error::InvalidImage("a damaged JPEG");
    let (mut at, mut inverted) = (2, false);
    loop {
        let &[0xFF, marker, high, low] = data.get(at..at + 4).ok_or_else(damaged)? else {
            return Err(damaged());
        };
        let length = usize::from(u16::from_be_bytes([high, low]));
        if marker == 0xEE && data.get(at + 4..at + 9) == Some(b"Adobe") {
            inverted = true;
        }
        // SOF0 to SOF15, but DHT, JPG and DAC share the range.
        if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            let frame = data.get(at + 4..at + 10).ok_or_else(damaged)?;
            (frame[0] == 8).ok_or(Error::InvalidImage("a JPEG of 8 bits a sample"))?;
            let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
            let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
            let components = frame[5];
            matches!(components, 1 | 3 | 4).ok_or_else(damaged)?;
            let kind = Kind::Jpeg { components, inverted: inverted && components == 4 };
            return Ok((kind, (width, height)));
        }
        at += 2 + length;
    }
}
