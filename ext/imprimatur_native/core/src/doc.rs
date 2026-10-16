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
mod table;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub use paragraph::{Paragraph, Run};
pub use table::Table;

use crate::{Error, Result};

/// Paper size; pages are portrait.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PageSize {
    #[default]
    A4,
    Letter,
}

impl PageSize {
    /// Width and height in PDF points (1/72 inch).
    pub(crate) const fn points(self) -> (f64, f64) {
        match self {
            Self::A4 => (595.276, 841.89),
            Self::Letter => (612.0, 792.0),
        }
    }

    /// Width and height in twentieths of a point, as Word measures pages.
    pub(crate) const fn twips(self) -> (u32, u32) {
        match self {
            Self::A4 => (11_906, 16_838),
            Self::Letter => (12_240, 15_840),
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
    PageBreak,
}

/// Content and page setup; render it with [`to_pdf`](Self::to_pdf) or
/// [`to_docx`](Self::to_docx).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Document {
    blocks: Vec<Block>,
    page_size: PageSize,
    page_numbers: bool,
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
