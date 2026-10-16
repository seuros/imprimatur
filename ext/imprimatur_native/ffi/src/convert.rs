//! Ruby values to cells.

use imprimatur::xlsx::{Cell, DateTime};
use magnus::prelude::*;
use magnus::{Error, Float, Integer, RClass, RString, Ruby, Value};

use crate::{OrRaise, domain_error};

/// The largest integer every `f64` neighbour of which is also an integer:
/// beyond it, Excel would silently store a different number.
const MAX_EXACT_INTEGER: u64 = 1 << 53;

/// Ruby classes looked up once per call, not once per cell.
pub struct Classes {
    formula: RClass,
    date: Option<RClass>,
}

impl Classes {
    pub fn load(ruby: &Ruby) -> Result<Self, Error> {
        let imprimatur: magnus::RModule = ruby.class_object().const_get("Imprimatur")?;
        // Date is only there once something has required 'date'.
        let date_defined: bool = ruby.class_object().funcall("const_defined?", ("Date",))?;
        let date = if date_defined { Some(ruby.class_object().const_get("Date")?) } else { None };
        Ok(Self { formula: imprimatur.const_get("Formula")?, date })
    }
}

/// `nil` is no cell.
pub fn to_cell(ruby: &Ruby, value: Value, classes: &Classes) -> Result<Option<Cell>, Error> {
    if value.is_nil() {
        return Ok(None);
    }
    if let Some(text) = RString::from_value(value) {
        return Ok(Some(Cell::Text(text.to_string()?)));
    }
    if let Some(float) = Float::from_value(value) {
        return Ok(Some(Cell::Number(float.to_f64())));
    }
    if let Some(integer) = Integer::from_value(value) {
        return integer_cell(ruby, integer).map(Some);
    }
    if value.is_kind_of(ruby.class_true_class()) {
        return Ok(Some(Cell::Bool(true)));
    }
    if value.is_kind_of(ruby.class_false_class()) {
        return Ok(Some(Cell::Bool(false)));
    }
    if value.is_kind_of(classes.formula) {
        let expression: String = value.funcall("expression", ())?;
        return Ok(Some(Cell::formula(expression)));
    }
    if value.is_kind_of(ruby.class_time()) {
        return time_cell(ruby, value).map(Some);
    }
    if let Some(date) = classes.date
        && value.is_kind_of(date)
    {
        // DateTime is a Date with a time of day; a plain Date has none.
        if value.respond_to("hour", false)? {
            let fraction: Value = value.funcall("sec_fraction", ())?;
            let nanosecond: u32 =
                fraction.funcall::<_, _, Value>("*", (1_000_000_000,))?.funcall("to_i", ())?;
            return date_cell(ruby, value, ["hour", "minute", "second"], nanosecond).map(Some);
        }
        return date_cell(ruby, value, [], 0).map(Some);
    }
    // BigDecimal, Rational: Excel keeps every number as a double anyway.
    if value.is_kind_of(ruby.class_numeric()) {
        let number: f64 = value.funcall("to_f", ())?;
        return Ok(Some(Cell::Number(number)));
    }
    // ActiveSupport::TimeWithZone is a Time only to Ruby's own is_a?; its
    // fields are the wall clock of its zone.
    if value.respond_to("is_a?", false)?
        && value.funcall::<_, _, bool>("is_a?", (ruby.class_time(),))?
    {
        return time_cell(ruby, value).map(Some);
    }
    Err(Error::new(
        ruby.exception_type_error(),
        format!("unsupported cell value {} ({})", value.inspect(), value.class().inspect()),
    ))
}

fn time_cell(ruby: &Ruby, value: Value) -> Result<Cell, Error> {
    let nanosecond: u32 = value.funcall("nsec", ())?;
    date_cell(ruby, value, ["hour", "min", "sec"], nanosecond)
}

/// The wall-clock fields of a `Time`, `Date` or `DateTime`, named by `time`.
fn date_cell<const N: usize>(
    ruby: &Ruby,
    value: Value,
    time: [&str; N],
    nanosecond: u32,
) -> Result<Cell, Error> {
    let year: i64 = value.funcall("year", ())?;
    let month: u8 = value.funcall("month", ())?;
    let day: u8 = value.funcall("day", ())?;
    let mut clock = [0_u8; 3];
    for (field, method) in clock.iter_mut().zip(time) {
        *field = value.funcall(method, ())?;
    }
    let out_of_range = || {
        let error = imprimatur::Error::DateOutOfRange(value.inspect());
        domain_error(ruby, &error)
    };
    let year = u16::try_from(year).map_err(|_| out_of_range())?;
    let date = DateTime::date(year, month, day).or_raise(ruby)?;
    let [hour, minute, second] = clock;
    let date = date.at(hour, minute, second, nanosecond).or_raise(ruby)?;
    Ok(Cell::DateTime(date))
}

/// Integers become numbers only when the `f64` holds them exactly.
fn integer_cell(ruby: &Ruby, integer: Integer) -> Result<Cell, Error> {
    let out_of_range = || {
        Error::new(
            ruby.exception_range_error(),
            format!(
                "{integer} cannot be stored exactly; Excel keeps numbers as doubles (up to 2**53)"
            ),
        )
    };
    let value = integer.to_i64().map_err(|_| out_of_range())?;
    if value.unsigned_abs() > MAX_EXACT_INTEGER {
        return Err(out_of_range());
    }
    #[expect(clippy::cast_precision_loss, reason = "|value| <= 2**53, so the cast is exact")]
    Ok(Cell::Number(value as f64))
}
