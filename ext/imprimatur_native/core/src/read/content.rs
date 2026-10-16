//! What a read finds, before it is printed: pages of blocks for a PDF,
//! sheets of cells for a workbook, blocks for a Word document. Markdown is
//! one rendering of it ([`Content::to_markdown`]); search and JSON output
//! work on it directly.

#[cfg(test)]
mod tests;

use std::ops::Range;
use std::sync::Arc;

use super::span;
use crate::xlsx::push_cell_reference;
use crate::xlsx::{DateTime, cell_reference};

/// A document's content, by format.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Content {
    Pdf(Vec<Page>),
    Xlsx(Vec<Sheet>),
    Docx(Vec<Block>),
}

impl Content {
    /// The document's format.
    #[must_use]
    pub const fn format(&self) -> super::Format {
        match self {
            Self::Pdf(_) => super::Format::Pdf,
            Self::Xlsx(_) => super::Format::Xlsx,
            Self::Docx(_) => super::Format::Docx,
        }
    }
}

/// A PDF page: its blocks in reading order, and how many images it draws.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// From 1.
    pub number: u32,
    pub blocks: Vec<Placed>,
    pub images: u32,
}

/// A block and where it lies on its page.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub block: Block,
    /// `None` for text that does not run left to right across the page.
    pub bounds: Option<Bounds>,
}

/// A rectangle in points from the top-left corner of the page as shown
/// (its rotation applied).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

/// A unit of a document's flow.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Block {
    /// Level 1 is the largest.
    Heading {
        level: u8,
        text: Text,
    },
    /// Its lines separated by `\n`.
    Paragraph {
        text: Text,
    },
    /// `level` 0 is the outermost list.
    ListItem {
        level: u8,
        ordered: bool,
        text: Text,
    },
    Table(Grid),
}

impl Block {
    /// The block's text: a table's cells, each on a line.
    #[must_use]
    pub fn plain_text(&self) -> std::borrow::Cow<'_, str> {
        match self {
            Self::Heading { text, .. } | Self::Paragraph { text } | Self::ListItem { text, .. } => {
                std::borrow::Cow::Borrowed(&text.text)
            }
            Self::Table(grid) => {
                let mut joined = String::new();
                for cell in grid.cells() {
                    if !joined.is_empty() {
                        joined.push('\n');
                    }
                    joined.push_str(grid.text(cell));
                }
                std::borrow::Cow::Owned(joined)
            }
        }
    }
}

/// Text, and the stretches of it that are styled.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Text {
    pub text: String,
    /// Byte ranges of `text`, in order and apart. Readers leave whitespace
    /// at a stretch's edges out of it.
    pub marks: Vec<Mark>,
}

impl Text {
    pub fn plain(text: impl Into<String>) -> Self {
        Self { text: text.into(), marks: Vec::new() }
    }
}

/// A styled stretch of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    pub range: Range<usize>,
    pub bold: bool,
    pub italic: bool,
    pub link: Option<String>,
}

/// A workbook sheet: its cells, then its comments.
#[derive(Debug, Clone, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub hidden: bool,
    pub cells: Grid,
    pub comments: Vec<Note>,
}

/// A comment on a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// The cell's position, from 0.
    pub row: u32,
    pub column: u32,
    /// Shared by the notes of one author.
    pub author: Option<Arc<str>>,
    pub text: String,
}

impl Note {
    /// The cell's A1 reference: `B2`.
    #[must_use]
    pub fn reference(&self) -> String {
        reference(self.row, self.column)
    }
}

/// Appends the A1 reference of a sheet's cell to `out`.
pub fn push_reference(out: &mut String, row: u32, column: u32) {
    push_cell_reference(out, row, u16::try_from(column).unwrap_or(u16::MAX));
}

fn reference(row: u32, column: u32) -> String {
    cell_reference(row, u16::try_from(column).unwrap_or(u16::MAX))
}

/// What a cell holds. Its text, as shown, is the grid's
/// ([`Grid::text`]); the variants carry what the text does not.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Value {
    Text,
    Number(f64),
    Bool(bool),
    /// A number formatted as a date or a date and time.
    Date(DateTime),
    /// An error such as `#N/A`.
    Error,
    /// A formula without a computed value: its text is `=FORMULA`.
    Formula,
}

/// A cell of a grid: its position, from 0, and its value.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub row: u32,
    pub column: u32,
    pub value: Value,
    text: Range<u32>,
}

impl Cell {
    /// The cell's A1 reference, which names it in a sheet: `B2`.
    #[must_use]
    pub fn reference(&self) -> String {
        reference(self.row, self.column)
    }
}

/// Cells by row and column, sparse and in row-major order. Their text
/// shares one buffer.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Grid {
    text: String,
    cells: Vec<Cell>,
    open: usize,
}

impl Grid {
    #[must_use]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// The text of a cell of this grid, as shown.
    #[must_use]
    pub fn text(&self, cell: &Cell) -> &str {
        span::slice(&self.text, &cell.text)
    }

    /// The cells row by row; rows without cells are left out.
    pub fn rows(&self) -> impl Iterator<Item = &[Cell]> {
        self.cells.chunk_by(|a, b| a.row == b.row)
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// The buffer to write the next cell's text into, then
    /// [`end_cell`](Self::end_cell) it.
    pub(crate) const fn buffer(&mut self) -> &mut String {
        self.open = self.text.len();
        &mut self.text
    }

    /// Ends the cell written since [`buffer`](Self::buffer); a cell with no
    /// text is dropped.
    pub(crate) fn end_cell(&mut self, row: u32, column: u32, value: Value) {
        if self.text[self.open..].trim().is_empty() {
            self.text.truncate(self.open);
        } else {
            let text = span::end(&self.text[..self.open])..span::end(&self.text);
            self.cells.push(Cell { row, column, value, text });
        }
    }

    #[cfg(test)]
    pub(crate) fn push(&mut self, row: u32, column: u32, text: &str) {
        self.buffer().push_str(text);
        self.end_cell(row, column, Value::Text);
    }

    /// Sorts cells written out of order; of two at one position, the first
    /// written stays.
    pub(crate) fn finish(mut self) -> Self {
        if !self.cells.is_sorted_by_key(|cell| (cell.row, cell.column)) {
            self.cells.sort_by_key(|cell| (cell.row, cell.column));
        }
        self.cells.dedup_by_key(|cell| (cell.row, cell.column));
        self
    }
}
