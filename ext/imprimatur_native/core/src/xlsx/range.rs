//! A1-style references and rectangular ranges, the way data validations and
//! comments address cells.

#[cfg(test)]
mod tests;

use std::fmt;
use std::str::FromStr;

use super::cell::push_cell_reference;
use super::worksheet::{MAX_COLUMNS, MAX_ROWS, check_column, check_row};
use crate::{Error, Result};

/// A rectangle of cells, zero-based and inclusive: `B2:D9` is rows 1 to 8 of
/// columns 1 to 3. Parsed from and displayed as A1 text.
///
/// ```
/// use imprimatur::xlsx::CellRange;
///
/// let range: CellRange = "D9:b2".parse()?;
/// assert_eq!(range.to_string(), "B2:D9");
/// assert_eq!(range.top_left(), (1, 1));
/// # Ok::<(), imprimatur::Error>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CellRange {
    top: u32,
    left: u16,
    bottom: u32,
    right: u16,
}

impl CellRange {
    /// The range between two `(row, column)` corners, in either order.
    ///
    /// # Errors
    /// [`Error::RowOutOfRange`] or [`Error::ColumnOutOfRange`].
    pub fn new(first: (u32, u16), last: (u32, u16)) -> Result<Self> {
        for (row, column) in [first, last] {
            check_row(row)?;
            check_column(column)?;
        }
        Ok(Self {
            top: first.0.min(last.0),
            left: first.1.min(last.1),
            bottom: first.0.max(last.0),
            right: first.1.max(last.1),
        })
    }

    /// A single cell.
    ///
    /// # Errors
    /// As [`new`](Self::new).
    pub fn cell(row: u32, column: u16) -> Result<Self> {
        Self::new((row, column), (row, column))
    }

    #[must_use]
    pub const fn top_left(&self) -> (u32, u16) {
        (self.top, self.left)
    }

    #[must_use]
    pub const fn bottom_right(&self) -> (u32, u16) {
        (self.bottom, self.right)
    }

    pub(crate) const fn overlaps(&self, other: &Self) -> bool {
        self.top <= other.bottom
            && other.top <= self.bottom
            && self.left <= other.right
            && other.left <= self.right
    }
}

impl FromStr for CellRange {
    type Err = Error;

    /// `B2`, or `B2:D9` with the corners in either order. No `$` markers,
    /// sheet names or whole-column ranges.
    fn from_str(text: &str) -> Result<Self> {
        let invalid = |_| Error::InvalidRange(text.to_owned());
        let (first, last) = text.split_once(':').unwrap_or((text, text));
        let first = parse_cell_reference(first).map_err(invalid)?;
        let last = parse_cell_reference(last).map_err(invalid)?;
        Self::new(first, last)
    }
}

impl fmt::Display for CellRange {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut text = String::with_capacity(21);
        push_cell_reference(&mut text, self.top, self.left);
        if (self.top, self.left) != (self.bottom, self.right) {
            text.push(':');
            push_cell_reference(&mut text, self.bottom, self.right);
        }
        out.write_str(&text)
    }
}

/// An A1 reference such as `B2`, its letters in either case, as a zero-based
/// `(row, column)`.
///
/// # Errors
/// [`Error::InvalidRange`] for anything else, or a cell beyond `XFD1048576`.
pub fn parse_cell_reference(reference: &str) -> Result<(u32, u16)> {
    let invalid = || Error::InvalidRange(reference.to_owned());
    let split = reference.find(|ch: char| !ch.is_ascii_alphabetic()).ok_or_else(invalid)?;
    let (letters, digits) = reference.split_at(split);
    let digits_valid = digits.bytes().all(|byte| byte.is_ascii_digit()) && !digits.starts_with('0');
    ((1..=3).contains(&letters.len()) && digits_valid).ok_or_else(invalid)?;
    // Bijective base 26: A is 1, Z 26, AA 27. Three letters stay under 2**15.
    let column = letters
        .bytes()
        .fold(0_u32, |sum, letter| sum * 26 + u32::from(letter.to_ascii_uppercase() - b'A') + 1);
    let column = u16::try_from(column - 1).ok().filter(|&column| column < MAX_COLUMNS);
    let row = digits.parse::<u32>().ok().filter(|&row| row <= MAX_ROWS);
    match (row, column) {
        (Some(row), Some(column)) => Ok((row - 1, column)),
        _ => Err(invalid()),
    }
}
