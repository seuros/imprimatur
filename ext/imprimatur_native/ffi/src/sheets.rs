//! `Imprimatur::Native.sheets`: a workbook's sheets as rows of Ruby values,
//! built from the cells directly instead of a hash per cell.

use imprimatur::read::{Cell, Content, Grid, Sheet, Value, content, json};
use imprimatur::xlsx::DateTime;
use magnus::prelude::*;
use magnus::{Error, IntoValue, RArray, RClass, RString, Ruby, Value as RubyValue};

use crate::domain_error;
use crate::read::without_gvl;

/// The largest whole `f64` an Integer can come from exactly.
const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;

/// Called by `Imprimatur::Spreadsheet.open`: `[name, hidden, rows, details]`
/// per sheet. The rows run from the first to the last with a value, each an
/// Array of values by column, `nil` where a cell is empty; the details are
/// the sheet's comments, columns, validations and, when `styles`, styles,
/// as JSON.
pub fn sheets_native(
    ruby: &Ruby,
    document: RString,
    max_input: Option<usize>,
    max_inflated: Option<u64>,
    timeout: Option<f64>,
    drop_repeated: bool,
    styles: bool,
) -> Result<RArray, Error> {
    let read =
        without_gvl(ruby, document, (max_input, max_inflated, timeout, drop_repeated), content)?;
    let Some(sheets) = read.sheets() else {
        let kind = if matches!(read, Content::Pdf(_)) { "a PDF" } else { "a Word document" };
        let error = imprimatur::Error::UnsupportedDocument(format!("{kind} is no spreadsheet"));
        return Err(domain_error(ruby, &error));
    };
    let dates = Dates::load(ruby)?;
    let out = ruby.ary_new_capa(sheets.len());
    for sheet in sheets {
        let details = json::sheet_details(sheet, json::Options { styles });
        let entry = [
            ruby.str_new(&sheet.name).into_value_with(ruby),
            sheet.hidden.into_value_with(ruby),
            rows(ruby, sheet, &dates)?.into_value_with(ruby),
            ruby.str_new(&serde_json::Value::Object(details).to_string()).into_value_with(ruby),
        ];
        out.push(ruby.ary_new_from_values(&entry))?;
    }
    Ok(out)
}

fn rows(ruby: &Ruby, sheet: &Sheet, dates: &Dates) -> Result<RArray, Error> {
    let grid = &sheet.cells;
    let count = grid.cells().last().map_or(0, |cell| cell.row + 1);
    let rows = ruby.ary_new_capa(usize::try_from(count).unwrap_or_default());
    let mut filled = grid.rows().peekable();
    for row in 0..count {
        let values = ruby.ary_new();
        if let Some(cells) =
            filled.next_if(|cells| cells.first().is_some_and(|cell| cell.row == row))
        {
            for cell in cells {
                let column = isize::try_from(cell.column).unwrap_or(isize::MAX);
                values.store(column, value(ruby, grid, cell, dates)?)?;
            }
        }
        rows.push(values)?;
    }
    Ok(rows)
}

/// A cell's value as Ruby has it. A formula with no computed result is nil.
fn value(ruby: &Ruby, grid: &Grid, cell: &Cell, dates: &Dates) -> Result<RubyValue, Error> {
    let text = grid.text(cell);
    Ok(match cell.value {
        Value::Number(number) => number_value(ruby, number, text),
        Value::Bool(on) => on.into_value_with(ruby),
        Value::Date(date) => dates.value(date)?,
        Value::Formula => ruby.qnil().as_value(),
        _ => ruby.str_new(text).as_value(),
    })
}

/// A whole number shown without a decimal point or a percent sign is an
/// Integer (`2`, `1,250`); any other a Float (`2.00`, `50%` as 0.5).
fn number_value(ruby: &Ruby, number: f64, shown: &str) -> RubyValue {
    let whole = number.fract() == 0.0 && number.abs() <= MAX_EXACT_INTEGER;
    if whole && !shown.contains(['.', '%']) {
        #[expect(clippy::cast_possible_truncation, reason = "a whole number within 2**53")]
        ruby.integer_from_i64(number as i64).as_value()
    } else {
        ruby.float_from_f64(number).as_value()
    }
}

/// `Date` and `DateTime`, looked up once per call.
struct Dates {
    date: RClass,
    date_time: RClass,
}

impl Dates {
    fn load(ruby: &Ruby) -> Result<Self, Error> {
        let object = ruby.class_object();
        Ok(Self { date: object.const_get("Date")?, date_time: object.const_get("DateTime")? })
    }

    /// A `Date`, or a `DateTime` when there is a time of day.
    fn value(&self, date: DateTime) -> Result<RubyValue, Error> {
        let (year, month, day) = date.year_month_day();
        if !date.has_time() {
            return self.date.funcall("new", (year, month, day));
        }
        let (hour, minute, second) = date.time_of_day();
        self.date_time.funcall("new", (year, month, day, hour, minute, second))
    }
}
