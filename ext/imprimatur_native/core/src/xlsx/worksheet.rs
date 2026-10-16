#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use super::text::check_length;
use super::{Cell, CellRange, Comment, Format, Validation};
use crate::{Error, Result};

/// Rows in a sheet (Excel 2007 and later).
pub const MAX_ROWS: u32 = 1_048_576;
/// Columns in a sheet: `A` to `XFD`.
pub const MAX_COLUMNS: u16 = 16_384;
const MAX_FORMULA: usize = 8_192;
const MAX_COLUMN_WIDTH: f64 = 255.0;
const DATE_FORMAT: &str = "yyyy-mm-dd";
const DATE_TIME_FORMAT: &str = "yyyy-mm-dd hh:mm:ss";
/// Excel's limit on distinct cell formats in a workbook.
pub const MAX_FORMATS: usize = 64_000;

/// A written cell and its format: 0 for the default look, otherwise
/// `formats[style - 1]`.
#[derive(Debug)]
pub struct Entry {
    pub cell: Cell,
    pub style: u32,
}

/// One sheet's cells, kept sparse and in row-major order, with the formats
/// they use interned once each.
#[derive(Debug)]
pub struct Worksheet {
    name: String,
    rows: BTreeMap<u32, BTreeMap<u16, Entry>>,
    next_row: u32,
    /// Shared with `format_ids`, so each format is stored once.
    formats: Vec<Arc<Format>>,
    format_ids: HashMap<Arc<Format>, u32>,
    /// The style of an unformatted date, then of an unformatted date and time.
    date_styles: [Option<u32>; 2],
    widths: BTreeMap<u16, f64>,
    right_to_left: bool,
    validations: Vec<(CellRange, Validation)>,
    comments: BTreeMap<(u32, u16), Comment>,
}

impl Worksheet {
    pub(super) fn new(name: String) -> Self {
        Self {
            name,
            rows: BTreeMap::new(),
            next_row: 0,
            formats: Vec::new(),
            format_ids: HashMap::new(),
            date_styles: [None; 2],
            widths: BTreeMap::new(),
            right_to_left: false,
            validations: Vec::new(),
            comments: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets one cell, replacing what was there. Row and column are zero-based.
    ///
    /// # Errors
    /// Out-of-range positions, text or formulas over Excel's limits, and
    /// non-finite numbers.
    pub fn write(&mut self, row: u32, column: u16, value: impl Into<Cell>) -> Result<&mut Self> {
        self.write_cell(row, column, value.into(), None)
    }

    /// As [`write`](Self::write), with a format.
    ///
    /// # Errors
    /// As [`write`](Self::write).
    pub fn write_with_format(
        &mut self,
        row: u32,
        column: u16,
        value: impl Into<Cell>,
        format: &Format,
    ) -> Result<&mut Self> {
        self.write_cell(row, column, value.into(), Some(format))
    }

    /// Writes `cells` from column `A` of the row after the last one written.
    /// Nothing is written unless every cell is valid.
    ///
    /// # Errors
    /// As [`write`](Self::write); also when the sheet is full.
    pub fn append_row<C: Into<Cell>>(
        &mut self,
        cells: impl IntoIterator<Item = C>,
    ) -> Result<&mut Self> {
        self.append_sparse_row(cells.into_iter().map(|cell| Some(cell.into())), None)
    }

    /// As [`append_row`](Self::append_row), every cell with `format`.
    ///
    /// # Errors
    /// As [`append_row`](Self::append_row).
    pub fn append_row_with_format<C: Into<Cell>>(
        &mut self,
        cells: impl IntoIterator<Item = C>,
        format: &Format,
    ) -> Result<&mut Self> {
        self.append_sparse_row(cells.into_iter().map(|cell| Some(cell.into())), Some(format))
    }

    /// As [`append_row`](Self::append_row), where `None` leaves its column
    /// empty and `format`, if any, applies to every cell written.
    ///
    /// # Errors
    /// As [`append_row`](Self::append_row).
    pub fn append_sparse_row(
        &mut self,
        cells: impl IntoIterator<Item = Option<Cell>>,
        format: Option<&Format>,
    ) -> Result<&mut Self> {
        self.append_styled_row(cells.into_iter().map(|cell| (cell, format)))
    }

    /// As [`append_sparse_row`](Self::append_sparse_row), each cell with a
    /// format of its own.
    ///
    /// # Errors
    /// As [`append_row`](Self::append_row).
    pub fn append_styled_row<'f>(
        &mut self,
        cells: impl IntoIterator<Item = (Option<Cell>, Option<&'f Format>)>,
    ) -> Result<&mut Self> {
        let row = self.next_row;
        check_row(row)?;
        let mut valid = Vec::new();
        for (index, (cell, format)) in cells.into_iter().enumerate() {
            let column = u16::try_from(index)
                .ok()
                .filter(|&column| column < MAX_COLUMNS)
                .ok_or(Error::ColumnOutOfRange(index as u64))?;
            if let Some(cell) = cell {
                validate(&cell)?;
                valid.push((column, cell, format));
            }
        }
        // Formats are interned only once the whole row is known to be valid.
        let mut values = BTreeMap::new();
        for (column, cell, format) in valid {
            let style = self.style_for(&cell, format)?;
            values.insert(column, Entry { cell, style });
        }
        if !values.is_empty() {
            self.rows.insert(row, values);
        }
        self.next_row = row + 1;
        Ok(self)
    }

    /// Sets a column's width, in characters of the default font.
    ///
    /// # Errors
    /// [`Error::ColumnOutOfRange`], or [`Error::InvalidColumnWidth`] outside
    /// 0 to 255.
    pub fn set_column_width(&mut self, column: u16, width: f64) -> Result<&mut Self> {
        check_column(column)?;
        (width.is_finite() && (0.0..=MAX_COLUMN_WIDTH).contains(&width))
            .ok_or(Error::InvalidColumnWidth(width))?;
        self.widths.insert(column, width);
        Ok(self)
    }

    /// Lays the sheet out right to left: column `A` on the right, as Excel
    /// shows sheets for Arabic and Hebrew.
    pub const fn set_right_to_left(&mut self, right_to_left: bool) -> &mut Self {
        self.right_to_left = right_to_left;
        self
    }

    #[must_use]
    pub const fn is_right_to_left(&self) -> bool {
        self.right_to_left
    }

    /// Applies a rule to the cells of `range`, whether or not they hold values.
    ///
    /// # Errors
    /// [`Error::OverlappingValidation`] when the range shares a cell with
    /// one already validated: Excel applies a single rule per cell.
    pub fn add_validation(
        &mut self,
        range: CellRange,
        validation: Validation,
    ) -> Result<&mut Self> {
        if self.validations.iter().any(|(taken, _)| taken.overlaps(&range)) {
            return Err(Error::OverlappingValidation(range.to_string()));
        }
        self.validations.push((range, validation));
        Ok(self)
    }

    /// The rules added, in order, each with its range.
    #[must_use]
    pub fn validations(&self) -> &[(CellRange, Validation)] {
        &self.validations
    }

    /// Attaches a comment to a cell, replacing any it had. The cell need not
    /// hold a value.
    ///
    /// # Errors
    /// [`Error::RowOutOfRange`] or [`Error::ColumnOutOfRange`].
    pub fn set_comment(&mut self, row: u32, column: u16, comment: Comment) -> Result<&mut Self> {
        check_row(row)?;
        check_column(column)?;
        self.comments.insert((row, column), comment);
        Ok(self)
    }

    /// The comment on a cell, if it has one.
    #[must_use]
    pub fn comment(&self, row: u32, column: u16) -> Option<&Comment> {
        self.comments.get(&(row, column))
    }

    /// The cell at a position, if one was written.
    #[must_use]
    pub fn get(&self, row: u32, column: u16) -> Option<&Cell> {
        Some(&self.entry(row, column)?.cell)
    }

    /// The format of the cell at a position, if it has one.
    #[must_use]
    pub fn format_at(&self, row: u32, column: u16) -> Option<&Format> {
        let style = self.entry(row, column)?.style;
        self.formats.get(usize::try_from(style).ok()?.checked_sub(1)?).map(AsRef::as_ref)
    }

    /// The zero-based row an [`append_row`](Self::append_row) writes next.
    #[must_use]
    pub const fn next_row(&self) -> u32 {
        self.next_row
    }

    /// Rows that hold at least one cell, in order, each with its cells in
    /// column order.
    pub(crate) fn rows(&self) -> impl Iterator<Item = (u32, &BTreeMap<u16, Entry>)> {
        self.rows.iter().map(|(&row, cells)| (row, cells))
    }

    /// The formats cells refer to, `style - 1` indexed.
    pub(crate) fn formats(&self) -> &[Arc<Format>] {
        &self.formats
    }

    pub(crate) const fn column_widths(&self) -> &BTreeMap<u16, f64> {
        &self.widths
    }

    /// Comments by `(row, column)`, in row-major order.
    pub(crate) const fn comments(&self) -> &BTreeMap<(u32, u16), Comment> {
        &self.comments
    }

    fn entry(&self, row: u32, column: u16) -> Option<&Entry> {
        self.rows.get(&row)?.get(&column)
    }

    fn write_cell(
        &mut self,
        row: u32,
        column: u16,
        cell: Cell,
        format: Option<&Format>,
    ) -> Result<&mut Self> {
        check_row(row)?;
        check_column(column)?;
        validate(&cell)?;
        let style = self.style_for(&cell, format)?;
        self.rows.entry(row).or_default().insert(column, Entry { cell, style });
        self.next_row = self.next_row.max(row + 1);
        Ok(self)
    }

    /// The style id for a cell: dates without a number format get one, so
    /// they do not show as bare serial numbers.
    fn style_for(&mut self, cell: &Cell, format: Option<&Format>) -> Result<u32> {
        if let Cell::DateTime(date) = cell
            && !format.is_some_and(Format::has_number_format)
        {
            let slot = usize::from(date.has_time());
            // The common case, a date with no format at all, is cached.
            if format.is_none()
                && let Some(id) = self.date_styles[slot]
            {
                return Ok(id);
            }
            let code = if date.has_time() { DATE_TIME_FORMAT } else { DATE_FORMAT };
            let mut dated = format.cloned().unwrap_or_default();
            dated.number_code = Some(code.to_owned());
            let id = self.intern(&dated)?;
            if format.is_none() {
                self.date_styles[slot] = Some(id);
            }
            return Ok(id);
        }
        match format {
            Some(format) if *format != Format::default() => self.intern(format),
            _ => Ok(0),
        }
    }

    fn intern(&mut self, format: &Format) -> Result<u32> {
        if let Some(&id) = self.format_ids.get(format) {
            return Ok(id);
        }
        let id = u32::try_from(self.formats.len() + 1)
            .ok()
            .filter(|_| self.formats.len() < MAX_FORMATS)
            .ok_or(Error::TooManyFormats)?;
        let shared = Arc::new(format.clone());
        self.formats.push(Arc::clone(&shared));
        self.format_ids.insert(shared, id);
        Ok(id)
    }
}

pub(super) fn check_row(row: u32) -> Result<()> {
    (row < MAX_ROWS).ok_or(Error::RowOutOfRange(row.into()))
}

pub(super) fn check_column(column: u16) -> Result<()> {
    (column < MAX_COLUMNS).ok_or(Error::ColumnOutOfRange(column.into()))
}

fn validate(cell: &Cell) -> Result<()> {
    match cell {
        Cell::Text(text) => check_length(text),
        Cell::Formula(formula) => {
            let length = formula.chars().count();
            (length <= MAX_FORMULA).ok_or(Error::FormulaTooLong(length))
        }
        Cell::Number(number) => number.is_finite().ok_or(Error::NonFiniteNumber(*number)),
        Cell::Bool(_) | Cell::DateTime(_) => Ok(()),
    }
}
