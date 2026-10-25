//! A flowing document (headings, paragraphs, tables, page breaks) that
//! renders to PDF and DOCX from the same content.
//!
//! ```
//! use imprimatur::doc::{Document, Paragraph, Run, Table};
//! use imprimatur::style::{Align, Color};
//!
//! let mut doc = Document::new();
//! doc.heading(1, "Quarterly report")?;
//! doc.push_paragraph(
//!     Paragraph::new()
//!         .text("Revenue was ")
//!         .run(Run::new("up 12%").bold().color(Color::rgb(0x1F_7A_1F)))
//!         .text(" this quarter.")
//!         .align(Align::Left),
//! )?;
//! doc.table(Table::new([["Region", "Q1"], ["North", "12.5"]]).header())?;
//! let pdf = doc.to_pdf()?;
//! let docx = doc.to_docx()?;
//! assert!(pdf.starts_with(b"%PDF-") && docx.starts_with(b"PK"));
//! # Ok::<(), imprimatur::Error>(())
//! ```

mod paragraph;
pub(crate) mod picture;
mod size;
mod symbol;
mod table;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub use paragraph::{Paragraph, Run};
pub use picture::Picture;
pub use symbol::Symbol;
pub use table::Table;

use crate::script::Counts;
use crate::{Error, Result};

/// Paper size; A4 and Letter pages are portrait.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
    /// Width and height in hundredths of a point; see [`PageSize::custom`].
    Custom(u32, u32),
}

impl PageSize {
    /// A page of this width and height, in points (1/72 inch): 36 to 1584
    /// (half an inch to 22 inches), as both PDF and Word take them.
    ///
    /// # Errors
    /// [`Error::InvalidPageSize`] outside that range.
    pub fn custom(width: f64, height: f64) -> Result<Self> {
        let side = |points: f64| {
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "36 to 1584"
            )]
            (36.0..=1584.0).contains(&points).then(|| (points * 100.0).round() as u32)
        };
        side(width)
            .zip(side(height))
            .map(|(width, height)| Self::Custom(width, height))
            .ok_or(Error::InvalidPageSize(width, height))
    }

    /// Width and height in PDF points (1/72 inch).
    pub(crate) fn points(self) -> (f64, f64) {
        match self {
            Self::A4 => (595.276, 841.89),
            Self::Letter => (612.0, 792.0),
            Self::Custom(width, height) => (f64::from(width) / 100.0, f64::from(height) / 100.0),
        }
    }

    /// Width and height in twentieths of a point, as Word measures pages.
    pub(crate) const fn twips(self) -> (u32, u32) {
        match self {
            Self::A4 => (11_906, 16_838),
            Self::Letter => (12_240, 15_840),
            Self::Custom(width, height) => (width / 5, height / 5),
        }
    }
}

/// One piece of a document, in reading order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Block {
    Heading { level: u8, text: String },
    Paragraph(Paragraph),
    Table(Table),
    Picture(Picture),
    Symbol(Symbol),
    PageBreak,
}

/// Content and page setup; render it with [`to_pdf`](Self::to_pdf) or
/// [`to_docx`](Self::to_docx).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    blocks: Vec<Block>,
    page_size: PageSize,
    page_numbers: bool,
    /// Hundredths of a point; one inch when unset.
    margin: Option<u32>,
    /// Half points; 11 points when unset.
    font_size: Option<u16>,
}

impl Document {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub const fn set_page_size(&mut self, size: PageSize) -> &mut Self {
        self.page_size = size;
        self
    }

    /// The margin on every side, in points: one inch (72) unless set.
    ///
    /// # Errors
    /// [`Error::InvalidMargin`] outside 0 to 144 points.
    pub fn set_margin(&mut self, points: f64) -> Result<&mut Self> {
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "0 to 144")]
        let margin = (0.0..=144.0)
            .contains(&points)
            .then(|| (points * 100.0).round() as u32)
            .ok_or(Error::InvalidMargin(points))?;
        self.margin = Some(margin);
        Ok(self)
    }

    /// The margin on every side, in points.
    #[must_use]
    pub fn margin(&self) -> f64 {
        self.margin.map_or(72.0, |margin| f64::from(margin) / 100.0)
    }

    /// Body text size in points, 11 unless set.
    ///
    /// # Errors
    /// [`Error::InvalidDocumentFontSize`] outside 1 to 1638, or between half points.
    pub fn set_font_size(&mut self, points: f64) -> Result<&mut Self> {
        let half_points = points * 2.0;
        let valid = (1.0..=1638.0).contains(&points) && half_points.fract() == 0.0;
        valid.ok_or(Error::InvalidDocumentFontSize(points))?;
        #[expect(clippy::cast_possible_truncation, reason = "a whole number from 2 to 3276")]
        #[expect(clippy::cast_sign_loss, reason = "a whole number from 2 to 3276")]
        let half_points = half_points as u16;
        self.font_size = Some(half_points);
        Ok(self)
    }

    /// Body text size in points.
    #[must_use]
    pub fn font_size(&self) -> f64 {
        f64::from(self.font_size.unwrap_or(22)) / 2.0
    }

    /// The width and height between the margins, in points.
    ///
    /// # Errors
    /// [`Error::InvalidMargin`] when the margins leave no room.
    pub(crate) fn room(&self) -> Result<(f64, f64)> {
        let (width, height) = self.page_size.points();
        let margin = self.margin();
        let room = (2.0f64.mul_add(-margin, width), 2.0f64.mul_add(-margin, height));
        (room.0 >= 1.0 && room.1 >= 1.0).then_some(room).ok_or(Error::InvalidMargin(margin))
    }

    /// A picture, in the flow of the text.
    pub fn picture(&mut self, picture: Picture) -> &mut Self {
        self.blocks.push(Block::Picture(picture));
        self
    }

    /// A barcode, in the flow of the text: a [`Barcode`](crate::barcode::Barcode)
    /// at its usual size, or a [`Symbol`] sized and aligned.
    pub fn barcode(&mut self, symbol: impl Into<Symbol>) -> &mut Self {
        self.blocks.push(Block::Symbol(symbol.into()));
        self
    }

    /// Numbers pages `1 / 3` at the bottom of each one.
    pub const fn set_page_numbers(&mut self, enabled: bool) -> &mut Self {
        self.page_numbers = enabled;
        self
    }

    /// A heading, level 1 (the title) to 3.
    ///
    /// # Errors
    /// [`Error::InvalidHeadingLevel`], or [`Error::ControlCharacter`] in the text.
    pub fn heading(&mut self, level: u8, text: impl Into<String>) -> Result<&mut Self> {
        (1..=3).contains(&level).ok_or(Error::InvalidHeadingLevel(level))?;
        let text = text.into();
        validate_text(&text)?;
        self.blocks.push(Block::Heading { level, text });
        Ok(self)
    }

    /// A paragraph of plain text. `\n` breaks the line, `\t` is a tab.
    ///
    /// # Errors
    /// [`Error::ControlCharacter`] for any other control character.
    pub fn paragraph(&mut self, text: impl Into<String>) -> Result<&mut Self> {
        self.push_paragraph(Paragraph::new().text(text))
    }

    /// # Errors
    /// [`Error::ControlCharacter`] in any run.
    pub fn push_paragraph(&mut self, paragraph: Paragraph) -> Result<&mut Self> {
        for run in &paragraph.runs {
            validate_text(&run.text)?;
        }
        self.blocks.push(Block::Paragraph(paragraph));
        Ok(self)
    }

    /// # Errors
    /// [`Error::EmptyTable`] without a row or a column, and
    /// [`Error::ControlCharacter`] in any cell.
    pub fn table(&mut self, table: Table) -> Result<&mut Self> {
        (table.columns() > 0).ok_or(Error::EmptyTable)?;
        for cell in table.rows.iter().flatten() {
            validate_text(cell)?;
        }
        self.blocks.push(Block::Table(table));
        Ok(self)
    }

    pub fn page_break(&mut self) -> &mut Self {
        self.blocks.push(Block::PageBreak);
        self
    }

    /// Whether most of the document's letters are right to left: its pages
    /// are then laid out right to left.
    pub(crate) fn reads_right_to_left(&self) -> bool {
        let mut counts = Counts::default();
        for block in &self.blocks {
            match block {
                Block::Heading { text, .. } => counts.add(text),
                Block::Paragraph(paragraph) => {
                    paragraph.runs.iter().for_each(|run| counts.add(&run.text));
                }
                Block::Table(table) => table.count(&mut counts),
                Block::Picture(_) | Block::Symbol(_) | Block::PageBreak => {}
            }
        }
        counts.right_to_left()
    }

    #[must_use]
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    #[must_use]
    pub const fn page_size(&self) -> PageSize {
        self.page_size
    }

    #[must_use]
    pub const fn has_page_numbers(&self) -> bool {
        self.page_numbers
    }

    /// # Errors
    /// [`Error::UnsupportedCharacter`] for text the PDF standard fonts cannot
    /// encode, [`Error::RowTooTall`], or [`Error::Io`] when `out` fails.
    pub fn write_pdf<W: Write>(&self, out: W) -> Result<W> {
        crate::pdf::write(self, out)
    }

    /// # Errors
    /// As [`write_pdf`](Self::write_pdf).
    pub fn to_pdf(&self) -> Result<Vec<u8>> {
        self.write_pdf(Vec::new())
    }

    /// # Errors
    /// As [`write_pdf`](Self::write_pdf).
    pub fn save_pdf(&self, path: impl AsRef<Path>) -> Result<()> {
        self.write_pdf(BufWriter::new(File::create(path)?))?;
        Ok(())
    }

    /// # Errors
    /// [`Error::Io`] when `out` fails.
    pub fn write_docx<W: Write>(&self, out: W) -> Result<W> {
        crate::docx::write(self, out)
    }

    /// # Errors
    /// As [`write_docx`](Self::write_docx).
    pub fn to_docx(&self) -> Result<Vec<u8>> {
        self.write_docx(Vec::new())
    }

    /// # Errors
    /// As [`write_docx`](Self::write_docx).
    pub fn save_docx(&self, path: impl AsRef<Path>) -> Result<()> {
        self.write_docx(BufWriter::new(File::create(path)?))?;
        Ok(())
    }
}

/// Newlines and tabs mean something in every format; other control
/// characters mean nothing in any of them.
fn validate_text(text: &str) -> Result<()> {
    let invalid =
        |ch: char| (ch.is_control() && ch != '\n' && ch != '\t') || crate::xml::is_forbidden(ch);
    text.chars()
        .find(|&ch| invalid(ch))
        .map_or(Ok(()), |ch| Err(Error::ControlCharacter(u32::from(ch))))
}
