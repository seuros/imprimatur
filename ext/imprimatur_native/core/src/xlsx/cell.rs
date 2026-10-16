#[cfg(test)]
mod tests;

use std::fmt::Write as _;

use super::DateTime;

/// A cell value.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Cell {
    Text(String),
    Number(f64),
    Bool(bool),
    /// A formula without its leading `=`, such as `SUM(A1:A3)`. Excel
    /// computes it when the workbook opens.
    Formula(String),
    /// A date, stored as its serial number. Without a number format of its
    /// own the cell gets `yyyy-mm-dd`, or `yyyy-mm-dd hh:mm:ss` with a time.
    DateTime(DateTime),
}

impl Cell {
    /// A formula cell. A leading `=` is accepted and dropped.
    pub fn formula(expression: impl Into<String>) -> Self {
        let mut expression = expression.into();
        if expression.starts_with('=') {
            expression.remove(0);
        }
        Self::Formula(expression)
    }
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self::Text(text)
    }
}

impl From<bool> for Cell {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<DateTime> for Cell {
    fn from(value: DateTime) -> Self {
        Self::DateTime(value)
    }
}

impl From<f64> for Cell {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

/// Integers that convert to `f64` without loss; Excel stores every number as one.
macro_rules! lossless_numbers {
    ($($int:ty),*) => {$(
        impl From<$int> for Cell {
            fn from(value: $int) -> Self {
                Self::Number(f64::from(value))
            }
        }
    )*};
}
lossless_numbers!(f32, i8, i16, i32, u8, u16, u32);

/// The column's letters: 0 is `A`, 25 is `Z`, 26 is `AA`.
#[must_use]
pub fn column_name(column: u16) -> String {
    let mut name = String::with_capacity(3);
    push_column_name(&mut name, column);
    name
}

/// A1-style reference for a zero-based row and column.
#[must_use]
pub fn cell_reference(row: u32, column: u16) -> String {
    let mut reference = String::with_capacity(10);
    push_cell_reference(&mut reference, row, column);
    reference
}

/// Appends the column's letters to `out`, without allocating.
pub fn push_column_name(out: &mut String, column: u16) {
    const ALPHABET: &[u8; 26] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    // XFD, the last column, is three letters.
    let mut letters = [0_u8; 3];
    let mut length = 0;
    let mut rest = usize::from(column) + 1;
    while rest > 0 && length < letters.len() {
        letters[length] = ALPHABET[(rest - 1) % 26];
        length += 1;
        rest = (rest - 1) / 26;
    }
    out.extend(letters[..length].iter().rev().map(|&letter| char::from(letter)));
}

/// Appends an A1-style reference to `out`, without allocating.
pub fn push_cell_reference(out: &mut String, row: u32, column: u16) {
    push_column_name(out, column);
    // Writing to a String cannot fail.
    let _ = write!(out, "{}", u64::from(row) + 1);
}
