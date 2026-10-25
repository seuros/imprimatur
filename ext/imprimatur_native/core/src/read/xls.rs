//! Legacy Excel workbooks (`.xls`): BIFF8 (Excel 97 to 2003) and BIFF5
//! (Excel 5.0 and 95), the `Workbook` or `Book` stream of a compound file,
//! a sequence of records. Cell values come from the cells' records (shared
//! or inline strings, numbers, RK numbers, booleans, errors, formula
//! results) and show through their number formats as in XLSX. Comments,
//! charts and macro sheets are left out.

mod strings;
#[cfg(test)]
mod tests;

use super::Budget;
use super::content::{Grid, Sheet, Value};
use super::numbers::{Shown, built_in, classify, push_number};
use super::ole::Ole;
use crate::bytes::{u16_le as u16_at, u32_le as u32_at};
use crate::{Error, Result};
use strings::Encoding;

const BOF: u16 = 0x0809;
const EOF: u16 = 0x000A;
const CONTINUE: u16 = 0x003C;
/// BIFF8, the version of Excel 97 and later; BIFF5, of Excel 5.0 and 95.
const BIFF8: u16 = 0x0600;
const BIFF5: u16 = 0x0500;

fn malformed(reason: &str) -> Error {
    Error::malformed("XLS", reason)
}

/// The workbook's sheets, in order, its chart and macro sheets left out.
///
/// # Errors
/// [`Error::UnsupportedDocument`] for a compound file that is no workbook,
/// one older than Excel 5.0 or in a code page other than Western European,
/// [`Error::PasswordRequired`] for an encrypted one, [`Error::Malformed`]
/// for a damaged one.
pub fn content(ole: &Ole<'_>, budget: &Budget<'_>) -> Result<Vec<Sheet>> {
    let stream = match ole.stream("Workbook", budget)? {
        Some(stream) => stream,
        None => ole.stream("Book", budget)?.ok_or_else(|| {
            Error::UnsupportedDocument(
                "an Office file that is not a workbook (a legacy .doc?)".to_owned(),
            )
        })?,
    };
    let book = globals(&stream, budget)?;
    let mut scratch = String::new();
    let mut sheets = Vec::with_capacity(book.sheets.len());
    for listed in &book.sheets {
        let cells = cells(&stream, listed.offset, &book, &mut scratch, budget)?;
        sheets.push(Sheet {
            name: listed.name.clone(),
            hidden: listed.hidden,
            cells,
            ..Sheet::default()
        });
    }
    Ok(sheets)
}

/// One record: its type and its body, `CONTINUE` records left apart.
fn records(data: &[u8], mut at: usize) -> impl Iterator<Item = (u16, &[u8])> {
    std::iter::from_fn(move || {
        let header = data.get(at..at + 4)?;
        let kind = u16::from_le_bytes([header[0], header[1]]);
        let length = usize::from(u16::from_le_bytes([header[2], header[3]]));
        let body = data.get(at + 4..at + 4 + length)?;
        at += 4 + length;
        Some((kind, body))
    })
}

/// A worksheet the workbook lists.
struct Listed {
    name: String,
    hidden: bool,
    offset: usize,
}

/// What the workbook's global records say: its sheets, its shared strings,
/// how each cell format shows numbers, and its date system.
struct Book {
    sheets: Vec<Listed>,
    strings: Vec<String>,
    shown: Vec<Shown>,
    date1904: bool,
    encoding: Encoding,
}

impl Book {
    /// How the cell format `xf` shows numbers.
    fn shown(&self, xf: Option<u16>) -> Shown {
        xf.and_then(|xf| self.shown.get(usize::from(xf))).copied().unwrap_or(Shown::General)
    }

    /// A string in the workbook's encoding.
    fn string(&self, bytes: &[u8], at: usize, wide_count: bool) -> String {
        strings::string(bytes, at, wide_count, self.encoding).unwrap_or_default()
    }
}

fn globals(stream: &[u8], budget: &Budget<'_>) -> Result<Book> {
    let mut all = records(stream, 0).peekable();
    let encoding = match all.next() {
        Some((BOF, body)) if u16_at(body, 0) == Some(BIFF8) => Encoding::Unicode,
        Some((BOF, body)) if u16_at(body, 0) == Some(BIFF5) => Encoding::Windows1252,
        Some((BOF, _)) => {
            return Err(Error::UnsupportedDocument(
                "a workbook older than Excel 5.0 (.xls); save it as .xlsx".into(),
            ));
        }
        _ => return Err(malformed("the workbook stream does not start with BOF")),
    };
    let mut book = Book {
        sheets: Vec::new(),
        strings: Vec::new(),
        shown: Vec::new(),
        date1904: false,
        encoding,
    };
    let mut formats: Vec<(u16, Shown)> = Vec::new();
    let mut format_ids = Vec::new();
    while let Some((kind, body)) = all.next() {
        budget.tick()?;
        match kind {
            EOF => break,
            // FILEPASS: the rest is encrypted.
            0x002F => return Err(Error::PasswordRequired),
            0x0022 => book.date1904 = u16_at(body, 0) == Some(1),
            // CODEPAGE: BIFF5's strings are bytes of it; Western European
            // (or ASCII) only, rather than misread text.
            0x0042 if encoding == Encoding::Windows1252 => {
                let page = u16_at(body, 0).unwrap_or(1252);
                if !matches!(page, 1252 | 367 | 0x8001) {
                    return Err(Error::UnsupportedDocument(format!(
                        "an Excel 5.0/95 workbook in code page {page} (.xls); save it as .xlsx"
                    )));
                }
            }
            0x041E => {
                if let Some(id) = u16_at(body, 0) {
                    let code = book.string(body, 2, encoding == Encoding::Unicode);
                    formats.push((id, classify(&code)));
                }
            }
            0x00E0 => format_ids.push(u16_at(body, 2).unwrap_or(0)),
            0x0085 => {
                let (Some(offset), Some(&state), Some(&kind)) =
                    (u32_at(body, 0), body.get(4), body.get(5))
                else {
                    continue;
                };
                // Worksheets only: macro sheets, charts and modules hold no table.
                if kind == 0 {
                    let name = book.string(body, 6, false);
                    book.sheets.push(Listed {
                        name,
                        hidden: state & 3 != 0,
                        offset: offset as usize,
                    });
                }
            }
            0x00FC => {
                let mut parts = vec![body];
                while let Some(&(CONTINUE, more)) = all.peek() {
                    parts.push(more);
                    all.next();
                }
                book.strings = strings::shared(&parts, budget)?;
            }
            _ => {}
        }
    }
    book.shown = format_ids
        .iter()
        .map(|&id| {
            formats
                .iter()
                .find(|&&(custom, _)| custom == id)
                .map_or_else(|| built_in(u32::from(id)), |&(_, shown)| shown)
        })
        .collect();
    Ok(book)
}

/// An `RK` number: a 30-bit integer or the high bits of a double, either
/// perhaps divided by 100.
fn rk(value: u32) -> f64 {
    #[expect(clippy::cast_possible_wrap, reason = "the bits are a signed integer")]
    let number = if value & 2 != 0 {
        f64::from((value as i32) >> 2)
    } else {
        f64::from_bits(u64::from(value & 0xFFFF_FFFC) << 32)
    };
    if value & 1 != 0 { number / 100.0 } else { number }
}

/// An error code as Excel shows it.
const fn error_text(code: u8) -> &'static str {
    match code {
        0x00 => "#NULL!",
        0x07 => "#DIV/0!",
        0x0F => "#VALUE!",
        0x17 => "#REF!",
        0x1D => "#NAME?",
        0x24 => "#NUM!",
        _ => "#N/A",
    }
}

/// A worksheet's cells, from its BOF at `offset` to its EOF.
fn cells(
    stream: &[u8],
    offset: usize,
    book: &Book,
    scratch: &mut String,
    budget: &Budget<'_>,
) -> Result<Grid> {
    let mut grid = Grid::default();
    let mut all = records(stream, offset);
    if !matches!(all.next(), Some((BOF, _))) {
        return Err(malformed("a sheet that does not start with BOF"));
    }
    // A formula's string result follows it in a STRING record.
    let mut pending: Option<(u32, u32)> = None;
    for (kind, body) in all {
        budget.tick()?;
        let (Some(row), Some(column)) = (u16_at(body, 0), u16_at(body, 2)) else {
            if kind == EOF {
                break;
            }
            continue;
        };
        let (row, column) = (u32::from(row), u32::from(column));
        // NUMBER, RK and numeric FORMULA results, written after the match.
        let number = match kind {
            EOF => break,
            // LABELSST
            0x00FD => {
                let text = u32_at(body, 6).and_then(|index| book.strings.get(index as usize));
                text_cell(&mut grid, (row, column), text.map_or("", String::as_str));
                None
            }
            // LABEL, RSTRING
            0x0204 | 0x00D6 => {
                text_cell(&mut grid, (row, column), &book.string(body, 6, true));
                None
            }
            0x0203 => {
                body.get(6..14).map(|bits| f64::from_le_bytes(bits.try_into().unwrap_or([0; 8])))
            }
            0x027E => u32_at(body, 6).map(rk),
            // MULRK: (format, RK) pairs from the first column on.
            0x00BD => {
                let pairs = body.get(4..body.len().saturating_sub(2)).unwrap_or_default();
                for (index, pair) in (0..).zip(pairs.as_chunks::<6>().0) {
                    let value = rk(u32::from_le_bytes([pair[2], pair[3], pair[4], pair[5]]));
                    let shown = book.shown(Some(u16::from_le_bytes([pair[0], pair[1]])));
                    number_cell(
                        &mut grid,
                        (row, column + index),
                        value,
                        shown,
                        book.date1904,
                        scratch,
                    );
                }
                None
            }
            // BOOLERR
            0x0205 => {
                let (Some(&value), Some(&error)) = (body.get(6), body.get(7)) else { continue };
                flag_cell(&mut grid, (row, column), value, error != 0);
                None
            }
            // FORMULA: its cached result.
            0x0006 => {
                let Some(result) = body.get(6..14) else { continue };
                if result[6..8] == [0xFF, 0xFF] {
                    match result[0] {
                        0 => pending = Some((row, column)),
                        1 => flag_cell(&mut grid, (row, column), result[2], false),
                        2 => flag_cell(&mut grid, (row, column), result[2], true),
                        _ => {}
                    }
                    None
                } else {
                    Some(f64::from_le_bytes(result.try_into().unwrap_or([0; 8])))
                }
            }
            // STRING: the string result of the formula before it.
            0x0207 => {
                if let Some(at) = pending.take() {
                    text_cell(&mut grid, at, &book.string(body, 0, true));
                }
                None
            }
            _ => None,
        };
        if let Some(number) = number {
            let shown = book.shown(u16_at(body, 4));
            number_cell(&mut grid, (row, column), number, shown, book.date1904, scratch);
        }
    }
    Ok(grid.finish())
}

fn number_cell(
    grid: &mut Grid,
    (row, column): (u32, u32),
    number: f64,
    shown: Shown,
    date1904: bool,
    scratch: &mut String,
) {
    let value = push_number(grid.buffer(), number, shown, date1904, scratch);
    grid.end_cell(row, column, value);
}

fn text_cell(grid: &mut Grid, (row, column): (u32, u32), text: &str) {
    grid.buffer().push_str(text);
    grid.end_cell(row, column, Value::Text);
}

/// A boolean, or an error code.
fn flag_cell(grid: &mut Grid, (row, column): (u32, u32), value: u8, error: bool) {
    let (text, value) = match (error, value) {
        (true, code) => (error_text(code), Value::Error),
        (false, 0) => ("FALSE", Value::Bool(false)),
        (false, _) => ("TRUE", Value::Bool(true)),
    };
    grid.buffer().push_str(text);
    grid.end_cell(row, column, value);
}
