//! Documents in: the other direction. A PDF's text, an XLSX's sheets and a
//! DOCX's body, as [`Content`] to search or serialize, or as Markdown for
//! search indexes and language models.
//!
//! ```
//! use imprimatur::read::{Limits, markdown};
//! use imprimatur::xlsx::Workbook;
//!
//! let mut book = Workbook::new();
//! book.add_sheet("Sales")?.append_row(["Region", "Q1"])?.append_row(["North", "12.5"])?;
//! let text = markdown(&book.to_bytes()?, &Limits::default())?;
//! assert_eq!(text, "## Sales\n\n| Region | Q1 |\n| --- | --- |\n| North | 12.5 |\n");
//! # Ok::<(), imprimatur::Error>(())
//! ```

mod budget;
mod content;
mod docx;
mod image;
pub(crate) mod inflate;
#[cfg(feature = "json")]
pub mod json;
mod markdown;
mod package;
mod pdf;
mod span;
#[cfg(test)]
mod tests;
mod xlsx;

pub(crate) use budget::Budget;
pub use budget::Limits;
pub use content::{
    Block, Bounds, Cell, Content, Grid, Mark, Note, Page, Placed, Sheet, Text, Value,
};
pub use image::{Image, ImageFormat, ImageLocation};

use crate::zip::reader::ZipReader;
use crate::{Error, Result};
use package::Package;

/// The formats [`content`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    Pdf,
    Xlsx,
    Docx,
}

impl Format {
    /// `pdf`, `xlsx` or `docx`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Xlsx => "xlsx",
            Self::Docx => "docx",
        }
    }
}

/// Reads a document, its format told by its content: `%PDF-` in the first
/// kilobyte, or a zip whose main part is a workbook or a Word document
/// (macro-enabled ones and templates included).
///
/// # Errors
/// [`Error::UnsupportedDocument`] for other formats (legacy `.xls` and
/// `.doc`, binary `.xlsb`, presentations), [`Error::NoTextLayer`] for a PDF
/// of scanned pages, [`Error::PasswordRequired`] for a PDF that needs one,
/// [`Error::LimitExceeded`] and [`Error::Cancelled`] when the [`Limits`]
/// stop the read, and [`Error::Malformed`] for a corrupt document.
pub fn content(document: &[u8], limits: &Limits) -> Result<Content> {
    let budget = Budget::new(limits, document.len())?;
    if pdf::is_pdf(document) {
        return pdf::content(document, &budget).map(Content::Pdf);
    }
    let zip = zip(document)?;
    let package = Package::open(&zip, &budget)?;
    match package.format()? {
        Format::Xlsx => xlsx::content(&package, &budget).map(Content::Xlsx),
        Format::Docx => docx::content(&package, &budget).map(Content::Docx),
        Format::Pdf => Err(Error::UnsupportedDocument("a PDF inside a zip".to_owned())),
    }
}

/// Reads a document as Markdown: [`content`], then
/// [`Content::to_markdown`].
///
/// # Errors
/// As [`content`].
pub fn markdown(document: &[u8], limits: &Limits) -> Result<String> {
    content(document, limits).map(|content| content.to_markdown())
}

/// Calls `each` with every image in a document, as a file.
///
/// A PDF's images come once each, from the first page that draws them; a
/// Word or Excel package gives its media. A JPEG stays a JPEG; pixels a PDF
/// stores raw become PNG. Stops at the first error `each` returns.
///
/// # Errors
/// As [`content`] (but a PDF without text is no error here), and what
/// `each` returns.
pub fn images(
    document: &[u8],
    limits: &Limits,
    mut each: impl FnMut(Image) -> Result<()>,
) -> Result<()> {
    let budget = Budget::new(limits, document.len())?;
    if pdf::is_pdf(document) {
        return pdf::images(document, &budget, &mut each);
    }
    let zip = zip(document)?;
    let package = Package::open(&zip, &budget)?;
    let format = package.format()?;
    let sheets = match format {
        Format::Xlsx => xlsx::image_sheets(&package, &budget)?,
        _ => Vec::new(),
    };
    let folder = package.main().rsplit_once('/').map_or("", |(folder, _)| folder);
    let media = format!("{folder}/media/");
    let names: Vec<&str> = package.parts().filter(|name| name.starts_with(&media)).collect();
    for name in names {
        budget.check()?;
        let Some(data) = package.part(name)? else { continue };
        let format = ImageFormat::sniff(&data, name);
        let (width, height) = format.dimensions(&data).unzip();
        let sheet = sheets
            .iter()
            .find(|(part, _)| part.eq_ignore_ascii_case(name))
            .map(|(_, sheet)| sheet.clone());
        let location = ImageLocation::Part { name: name.to_owned(), sheet };
        each(Image { data, format, width, height, location })?;
    }
    Ok(())
}

/// A document that is not a PDF, as the zip an Office document is.
fn zip(document: &[u8]) -> Result<ZipReader<'_>> {
    if !document.starts_with(b"PK") {
        return Err(Error::UnsupportedDocument("not a PDF, XLSX or DOCX file".to_owned()));
    }
    ZipReader::new(document)
}
