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
mod numbers;
mod ods;
mod ole;
mod package;
pub mod pdf;
mod span;
#[cfg(test)]
mod tests;
mod xls;
mod xlsx;

pub(crate) use budget::Budget;
pub use budget::Limits;
pub use content::{
    Block, Bounds, Cell, Columns, Content, Grid, Look, Mark, Note, Page, Placed, Sheet, Styled,
    Text, Validation, Value,
};
#[cfg(test)]
pub(crate) use image::png;
pub use image::{Image, ImageFormat, ImageLocation};
pub(crate) use image::{PngColor, png_with_transparency};

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
    Ods,
    Xls,
}

impl Format {
    /// `pdf`, `xlsx`, `docx`, `ods` or `xls`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Xlsx => "xlsx",
            Self::Docx => "docx",
            Self::Ods => "ods",
            Self::Xls => "xls",
        }
    }
}

/// Reads a document, its format told by its content.
///
/// That is `%PDF-` in the first kilobyte; a compound file holding a legacy
/// Excel workbook (`.xls`); a
/// zip that is an ODF spreadsheet (`.ods`), or whose main part is
/// a workbook or a Word document (macro-enabled ones and templates
/// included).
///
/// # Errors
/// [`Error::UnsupportedDocument`] for other formats (legacy `.doc`, binary
/// `.xlsb`, presentations, ODF text), [`Error::PasswordRequired`] for a document that
/// needs one, [`Error::LimitExceeded`] and [`Error::Cancelled`] when the
/// [`Limits`] stop the read, and [`Error::Malformed`] for a corrupt document.
pub fn content(document: &[u8], limits: &Limits) -> Result<Content> {
    let budget = Budget::new(limits, document.len())?;
    if pdf::is_pdf(document) {
        return pdf::content(document, &budget).map(Content::Pdf);
    }
    if document.starts_with(&ole::SIGNATURE) {
        let ole = ole::Ole::open(document, &budget)?;
        return xls::content(&ole, &budget).map(Content::Xls);
    }
    let zip = zip(document)?;
    match ods::spreadsheet(&zip, &budget)? {
        Some(true) => return ods::content(&zip, &budget).map(Content::Ods),
        Some(false) => {
            return Err(Error::UnsupportedDocument(
                "an OpenDocument file that is not a spreadsheet".to_owned(),
            ));
        }
        None => {}
    }
    let package = Package::open(&zip, &budget)?;
    match package.format()? {
        Format::Xlsx => xlsx::content(&package, &budget).map(Content::Xlsx),
        Format::Docx => docx::content(&package, &budget).map(Content::Docx),
        other => Err(Error::UnsupportedDocument(format!("a {} inside a zip", other.as_str()))),
    }
}

/// Reads a document as Markdown: [`content`], then
/// [`Content::to_markdown`].
///
/// # Errors
/// As [`content`], and [`Error::NoTextLayer`] for a PDF without text.
pub fn markdown(document: &[u8], limits: &Limits) -> Result<String> {
    let content = content(document, limits)?;
    content.ensure_text_layer()?;
    Ok(content.to_markdown())
}

/// Calls `each` with every image in a document, as a file.
///
/// A PDF's images come once each, from the first page that draws them; a
/// Word, Excel or ODF package gives its media. A JPEG stays a
/// JPEG; pixels a PDF stores raw become PNG. Stops at the first error
/// `each` returns.
///
/// # Errors
/// As [`content`] (but a PDF without text is no error here, and a legacy
/// `.xls` is one), and what `each` returns.
pub fn images(
    document: &[u8],
    limits: &Limits,
    mut each: impl FnMut(Image) -> Result<()>,
) -> Result<()> {
    let budget = Budget::new(limits, document.len())?;
    if pdf::is_pdf(document) {
        return pdf::images(document, &budget, &mut each);
    }
    if document.starts_with(&ole::SIGNATURE) {
        return Err(Error::UnsupportedDocument("the images of a legacy Office file".to_owned()));
    }
    let zip = zip(document)?;
    if ods::spreadsheet(&zip, &budget)?.is_some() {
        return media(&zip, "Pictures/", &[], &budget, &mut each);
    }
    let package = Package::open(&zip, &budget)?;
    let sheets = match package.format()? {
        Format::Xlsx => xlsx::image_sheets(&package, &budget)?,
        _ => Vec::new(),
    };
    let folder = package.main().rsplit_once('/').map_or("", |(folder, _)| folder);
    media(&zip, &format!("{folder}/media/"), &sheets, &budget, &mut each)
}

/// The package's parts under `folder`, as images; `sheets` says which
/// sheet shows each, by part.
fn media(
    zip: &ZipReader<'_>,
    folder: &str,
    sheets: &[(String, String)],
    budget: &Budget<'_>,
    each: &mut impl FnMut(Image) -> Result<()>,
) -> Result<()> {
    let names: Vec<&str> = zip.names().filter(|name| name.starts_with(folder)).collect();
    for name in names {
        budget.check()?;
        let Some(data) = zip.read(name, budget)? else { continue };
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
        return Err(Error::UnsupportedDocument(
            "not a PDF, XLSX, XLS, ODS or DOCX file".to_owned(),
        ));
    }
    ZipReader::new(document)
}
