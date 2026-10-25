//! Barcodes in the flow: shapes in a PDF, a picture in DOCX.

use super::size;
use crate::Result;
use crate::barcode::{Barcode, Look, Shapes};
use crate::doc::Picture;
use crate::style::{Align, Color};

/// Pixels an inch a DOCX picture of a symbol has at least.
const DOCX_DPI: f64 = 300.0;

/// A [`Barcode`] drawn in a document's flow.
///
/// It is drawn at the width or height set (in points; a QR Code or Data
/// Matrix keeps square modules, a linear code's height is its bars'), or
/// else at its symbology's usual module size, never larger than the page
/// leaves room for. Its quiet zone is part of its size.
///
/// ```
/// use imprimatur::barcode::{Barcode, Level};
/// use imprimatur::doc::{Document, Symbol};
/// use imprimatur::style::Align;
///
/// let mut doc = Document::new();
/// let qr = Barcode::qr("https://example.com", Level::Medium)?;
/// doc.barcode(Symbol::new(qr).width(144.0)?.align(Align::Center));
/// doc.barcode(Barcode::ean13("950600013435")?);
/// assert!(doc.to_pdf()?.starts_with(b"%PDF-"));
/// # Ok::<(), imprimatur::Error>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Symbol {
    pub(crate) barcode: Barcode,
    width: Option<f64>,
    height: Option<f64>,
    pub(crate) align: Option<Align>,
    pub(crate) color: Color,
}

impl Symbol {
    #[must_use]
    pub const fn new(barcode: Barcode) -> Self {
        Self { barcode, width: None, height: None, align: None, color: Color::BLACK }
    }

    /// Its width in points, quiet zone included.
    ///
    /// # Errors
    /// [`Error::InvalidBarcode`](crate::Error::InvalidBarcode) outside 1 to
    /// 14 400 points.
    pub fn width(mut self, points: f64) -> Result<Self> {
        self.width = Some(self.points_in_range(points)?);
        Ok(self)
    }

    /// Its height in points: a linear code's bars, a QR Code or Data
    /// Matrix's whole (quiet zone included).
    ///
    /// # Errors
    /// [`Error::InvalidBarcode`](crate::Error::InvalidBarcode) outside 1 to
    /// 14 400 points.
    pub fn height(mut self, points: f64) -> Result<Self> {
        self.height = Some(self.points_in_range(points)?);
        Ok(self)
    }

    #[must_use]
    pub const fn align(mut self, align: Align) -> Self {
        self.align = Some(align);
        self
    }

    /// The bars' color; black unless set.
    #[must_use]
    pub const fn color(mut self, color: Color) -> Self {
        self.color = color;
        self
    }

    /// Its rectangles and the size they are drawn at, in points, fitted
    /// into `room` (the width and height between the margins).
    pub(crate) fn placed(&self, room: (f64, f64)) -> (Shapes, (f64, f64)) {
        let shapes = self.barcode.shapes(true, None);
        let (columns, rows) = shapes.extent();
        let module = self.barcode.default_module();
        let (width, height) = if self.barcode.symbology().is_linear() {
            let width = self.width.unwrap_or(columns * module);
            (width, self.height.unwrap_or(rows * width / columns))
        } else {
            let module = match (self.width, self.height) {
                (Some(width), Some(height)) => (width / columns).min(height / rows),
                (Some(width), None) => width / columns,
                (None, Some(height)) => height / rows,
                (None, None) => module,
            };
            (columns * module, rows * module)
        };
        (shapes, size::fit((width, height), room))
    }

    /// A PNG of it at 300 pixels an inch or more, for formats that take
    /// pictures only.
    pub(crate) fn picture(&self, room: (f64, f64)) -> Result<Picture> {
        let (shapes, (width, height)) = self.placed(room);
        let module = width / shapes.extent().0;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "1 to 64")]
        let scale = (module * DOCX_DPI / 72.0).ceil().clamp(1.0, 64.0) as u32;
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "bars, small")]
        let bar_height = (height / module).round().max(1.0) as u32;
        let look = Look {
            dark: self.color,
            light: Some(Color::WHITE),
            quiet_zone: true,
            bar_height: Some(bar_height),
        };
        let png = self.barcode.png(scale, &look)?;
        let picture = Picture::new(png)?.width(width)?.height(height)?;
        Ok(match self.align {
            Some(align) => picture.align(align),
            None => picture,
        })
    }

    fn points_in_range(&self, points: f64) -> Result<f64> {
        let invalid = || self.barcode.symbology().invalid("a size of 1 to 14 400 points");
        size::points_in_range(points).ok_or_else(invalid)
    }
}

impl From<Barcode> for Symbol {
    fn from(barcode: Barcode) -> Self {
        Self::new(barcode)
    }
}
