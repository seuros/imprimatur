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

/// A document's content, by format. Every workbook format holds sheets.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Content {
    Pdf(Vec<Page>),
    Xlsx(Vec<Sheet>),
    Docx(Vec<Block>),
    /// An `.ods` spreadsheet.
    Ods(Vec<Sheet>),
    /// A legacy Excel workbook (Excel 97 to 2003).
    Xls(Vec<Sheet>),
}

impl Content {
    /// Requires readable PDF text for text-oriented callers, while structured
    /// callers can still inspect scanned and blank pages.
    ///
    /// # Errors
    /// [`crate::Error::NoTextLayer`] for a PDF without readable blocks.
    pub fn ensure_text_layer(&self) -> crate::Result<()> {
        if let Self::Pdf(pages) = self
            && pages.iter().all(|page| {
                page.blocks.iter().all(|block| block.block.plain_text().trim().is_empty())
            })
        {
            return Err(crate::Error::NoTextLayer);
        }
        Ok(())
    }

    /// The document's format.
    #[must_use]
    pub const fn format(&self) -> super::Format {
        match self {
            Self::Pdf(_) => super::Format::Pdf,
            Self::Xlsx(_) => super::Format::Xlsx,
            Self::Docx(_) => super::Format::Docx,
            Self::Ods(_) => super::Format::Ods,
            Self::Xls(_) => super::Format::Xls,
        }
    }

    /// A workbook's sheets, whatever its format; `None` for documents.
    #[must_use]
    pub fn sheets(&self) -> Option<&[Sheet]> {
        match self {
            Self::Xlsx(sheets) | Self::Ods(sheets) | Self::Xls(sheets) => Some(sheets),
            Self::Pdf(_) | Self::Docx(_) => None,
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

/// A workbook sheet: its cells and comments and, from an XLSX workbook,
/// its column widths, data validations and cell formats.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {
    pub name: String,
    pub hidden: bool,
    pub cells: Grid,
    pub comments: Vec<Note>,
    pub columns: Vec<Columns>,
    pub validations: Vec<Validation>,
    /// The cells with a format of their own, empty ones included.
    pub styles: Vec<Styled>,
}

/// A run of columns, from 0, and their width in characters.
#[derive(Debug, Clone, PartialEq)]
pub struct Columns {
    pub first: u32,
    pub last: u32,
    pub width: Option<f64>,
    pub hidden: bool,
}

/// A data validation rule, as the workbook stores it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Validation {
    /// The cells it applies to: `B2:B100 D2`.
    pub range: String,
    /// `list`, `whole`, `decimal`, `date`, `time`, `textLength`, `custom`,
    /// or `none` for an input message alone.
    pub kind: String,
    pub operator: Option<String>,
    pub formula1: Option<String>,
    pub formula2: Option<String>,
    pub allow_blank: bool,
    /// Whether a list shows its in-cell drop-down arrow.
    pub dropdown: bool,
    /// `stop`, `warning` or `information`, when a wrong value is refused.
    pub alert: Option<String>,
    pub error_title: Option<String>,
    pub error: Option<String>,
    pub prompt_title: Option<String>,
    pub prompt: Option<String>,
}

/// A cell with a format of its own, from 0.
#[derive(Debug, Clone, PartialEq)]
pub struct Styled {
    pub row: u32,
    pub column: u32,
    pub look: Arc<Look>,
}

impl Styled {
    /// The cell's A1 reference: `B2`.
    #[must_use]
    pub fn reference(&self) -> String {
        reference(self.row, self.column)
    }
}

/// How a cell format shows a cell. Colors are ARGB (`FFFF0000`), and only
/// those given as such: theme and indexed colors are left out.
#[derive(Debug, Clone, Default, PartialEq)]
#[expect(clippy::struct_excessive_bools, reason = "a format's switches, as Excel keeps them")]
pub struct Look {
    pub font: String,
    /// In points.
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: Option<String>,
    pub fill: Option<String>,
    pub align: Option<String>,
    pub valign: Option<String>,
    pub wrap: bool,
    /// The left border's style: `thin`, `medium`, `thick`, `dashed`...
    pub border: Option<String>,
    pub number_format: String,
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
