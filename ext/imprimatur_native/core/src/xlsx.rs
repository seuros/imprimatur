//! `SpreadsheetML` (`.xlsx`) workbooks, per ECMA-376 Part 1.

mod cell;
mod comment;
mod date;
mod format;
mod package;
mod range;
mod styles;
#[cfg(test)]
mod tests;
pub(crate) mod text;
mod validation;
mod worksheet;

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub use crate::style::{Align, Color, TextDirection};
pub(crate) use cell::push_cell_reference;
pub use cell::{Cell, cell_reference, column_name};
pub use comment::Comment;
pub use date::DateTime;
pub use format::{Border, Format, VerticalAlign};
pub use range::{CellRange, parse_cell_reference};
pub use validation::{Alert, Validation};
pub use worksheet::{MAX_COLUMNS, MAX_ROWS, Worksheet};

use crate::{Error, Result};

/// The longest sheet name Excel accepts.
const MAX_SHEET_NAME: usize = 31;

/// A workbook: an ordered list of uniquely named worksheets.
#[derive(Debug, Default)]
pub struct Workbook {
    sheets: Vec<Worksheet>,
}

impl Workbook {
    #[must_use]
    pub const fn new() -> Self {
        Self { sheets: Vec::new() }
    }

    /// Adds a sheet after the existing ones and returns it for writing.
    ///
    /// # Errors
    /// [`Error::InvalidSheetName`] for a name Excel rejects, and
    /// [`Error::DuplicateSheetName`] when the name is taken, ignoring case.
    pub fn add_sheet(&mut self, name: impl Into<String>) -> Result<&mut Worksheet> {
        let name = name.into();
        validate_sheet_name(&name)?;
        let folded = name.to_lowercase();
        if self.sheets.iter().any(|sheet| sheet.name().to_lowercase() == folded) {
            return Err(Error::DuplicateSheetName(name));
        }
        Ok(self.sheets.push_mut(Worksheet::new(name)))
    }

    #[must_use]
    pub fn sheets(&self) -> &[Worksheet] {
        &self.sheets
    }

    pub fn sheet_mut(&mut self, index: usize) -> Option<&mut Worksheet> {
        self.sheets.get_mut(index)
    }

    /// Writes the `.xlsx` package to `out` and returns it.
    ///
    /// # Errors
    /// [`Error::EmptyWorkbook`] without sheets, [`Error::Io`] when `out` fails.
    pub fn write_to<W: Write>(&self, out: W) -> Result<W> {
        package::write(&self.sheets, out)
    }

    /// The `.xlsx` package as bytes.
    ///
    /// # Errors
    /// As [`write_to`](Self::write_to).
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.write_to(Vec::new())
    }

    /// Writes the `.xlsx` package to a file, replacing it if it exists.
    ///
    /// # Errors
    /// As [`write_to`](Self::write_to).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let file = File::create(path)?;
        self.write_to(BufWriter::new(file))?;
        Ok(())
    }
}

fn validate_sheet_name(name: &str) -> Result<()> {
    let invalid = |reason| Err(Error::InvalidSheetName { name: name.to_owned(), reason });
    if name.is_empty() {
        return invalid("it is empty");
    }
    // Excel counts UTF-16 code units.
    if name.encode_utf16().count() > MAX_SHEET_NAME {
        return invalid("it is longer than 31 characters");
    }
    if name.contains(['[', ']', ':', '*', '?', '/', '\\']) {
        return invalid("it contains one of [ ] : * ? / \\");
    }
    if name.chars().any(crate::xml::is_forbidden) {
        return invalid("it contains a control character");
    }
    if name.starts_with('\'') || name.ends_with('\'') {
        return invalid("it starts or ends with an apostrophe");
    }
    if name.eq_ignore_ascii_case("history") {
        return invalid("Excel reserves the name History");
    }
    Ok(())
}
