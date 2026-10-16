//! Ruby bindings for the imprimatur crate: `Imprimatur::Native::Workbook`
//! wraps an [`imprimatur::xlsx::Workbook`], `Imprimatur::Style` a
//! [`imprimatur::xlsx::Format`]. The Ruby DSL in `lib/imprimatur/xlsx.rb`
//! drives them; sheets cross as indices.

mod convert;
mod document;
mod read;
mod style;
mod validation;

use std::cell::{RefCell, RefMut};

use imprimatur::style::{Align, TextDirection};
use imprimatur::xlsx::{CellRange, Comment, Workbook, Worksheet, parse_cell_reference};
use magnus::{
    Error, ExceptionClass, RArray, RModule, RString, Ruby, TryConvert, Value, function, method,
    prelude::*,
};

use convert::{Classes, to_cell};
use style::Style;
use validation::Validation;

#[magnus::wrap(class = "Imprimatur::Native::Workbook", free_immediately, size)]
struct NativeWorkbook(RefCell<Workbook>);

impl NativeWorkbook {
    const fn new() -> Self {
        Self(RefCell::new(Workbook::new()))
    }

    /// Adds a sheet; returns its index for the other methods.
    fn add_sheet(ruby: &Ruby, rb_self: &Self, name: String) -> Result<usize, Error> {
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        book.add_sheet(name).or_raise(ruby)?;
        Ok(book.sheets().len() - 1)
    }

    /// Appends a row; `nil` leaves its column empty. `style` is `nil`, one
    /// Style for every cell, or an Array of them (or `nil`) by column.
    fn append_row(
        ruby: &Ruby,
        rb_self: &Self,
        sheet: usize,
        cells: RArray,
        style: Value,
    ) -> Result<(), Error> {
        // Convert before borrowing: a Formula's #expression or a Date's
        // #year is Ruby code that could call back into this workbook.
        let classes = Classes::load(ruby)?;
        let mut row = Vec::with_capacity(cells.len());
        for value in cells {
            row.push(to_cell(ruby, value, &classes)?);
        }
        let styles = row_styles(ruby, style, row.len())?;
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        let formats = styles.iter().map(|style| style.map(|style| &style.0));
        let cells = row.into_iter().zip(formats.chain(std::iter::repeat(None)));
        sheet_mut(ruby, &mut book, sheet)?.append_styled_row(cells).or_raise(ruby)?;
        Ok(())
    }

    /// Sets one cell at a zero-based row and column.
    fn write(
        ruby: &Ruby,
        rb_self: &Self,
        position: (usize, u32, u16),
        value: Value,
        style: Option<&Style>,
    ) -> Result<(), Error> {
        let (sheet, row, column) = position;
        let cell = to_cell(ruby, value, &Classes::load(ruby)?)?
            .ok_or_else(|| argument_error(ruby, "a cell cannot be set to nil"))?;
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        let sheet = sheet_mut(ruby, &mut book, sheet)?;
        let written = match style {
            Some(style) => sheet.write_with_format(row, column, cell, &style.0),
            None => sheet.write(row, column, cell),
        };
        written.or_raise(ruby)?;
        Ok(())
    }

    fn set_column_width(
        ruby: &Ruby,
        rb_self: &Self,
        sheet: usize,
        column: u16,
        width: f64,
    ) -> Result<(), Error> {
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        sheet_mut(ruby, &mut book, sheet)?.set_column_width(column, width).or_raise(ruby)?;
        Ok(())
    }

    /// Applies a validation to an A1 range such as `B2:B100`.
    fn add_validation(
        ruby: &Ruby,
        rb_self: &Self,
        sheet: usize,
        range: RString,
        validation: &Validation,
    ) -> Result<(), Error> {
        let range: CellRange = range.to_string()?.parse().or_raise(ruby)?;
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        sheet_mut(ruby, &mut book, sheet)?
            .add_validation(range, validation.0.clone())
            .or_raise(ruby)?;
        Ok(())
    }

    /// Attaches a comment to the cell at an A1 reference such as `C1`.
    fn set_comment(
        ruby: &Ruby,
        rb_self: &Self,
        (sheet, reference): (usize, String),
        text: String,
        author: Option<String>,
        visible: bool,
    ) -> Result<(), Error> {
        let (row, column) = parse_cell_reference(&reference).or_raise(ruby)?;
        let mut comment = Comment::new(text).or_raise(ruby)?.visible(visible);
        if let Some(author) = author {
            comment = comment.author(author);
        }
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        sheet_mut(ruby, &mut book, sheet)?.set_comment(row, column, comment).or_raise(ruby)?;
        Ok(())
    }

    fn set_right_to_left(
        ruby: &Ruby,
        rb_self: &Self,
        sheet: usize,
        right_to_left: bool,
    ) -> Result<(), Error> {
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        sheet_mut(ruby, &mut book, sheet)?.set_right_to_left(right_to_left);
        Ok(())
    }

    /// The zero-based row the next append writes.
    fn next_row(ruby: &Ruby, rb_self: &Self, sheet: usize) -> Result<u32, Error> {
        let mut book = borrow(ruby, &rb_self.0, "workbook")?;
        Ok(sheet_mut(ruby, &mut book, sheet)?.next_row())
    }

    /// The `.xlsx` package as a binary String, written straight into it.
    fn render(ruby: &Ruby, rb_self: &Self) -> Result<RString, Error> {
        let out = RubyBytes(ruby.str_buf_new(0));
        let book = borrow(ruby, &rb_self.0, "workbook")?;
        Ok(book.write_to(out).or_raise(ruby)?.0)
    }

    /// Writes the package straight to `path`, without a Ruby String copy.
    fn save(ruby: &Ruby, rb_self: &Self, path: String) -> Result<(), Error> {
        borrow(ruby, &rb_self.0, "workbook")?.save(path).or_raise(ruby)
    }
}

/// `io::Write` into a binary Ruby String: rendered files land in Ruby memory
/// directly, not in an intermediate buffer that is then copied.
struct RubyBytes(RString);

impl std::io::Write for RubyBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.cat(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Mutable access to a wrapped value; Ruby code that calls back into the
/// same object mid-call gets a `RuntimeError`, not a panic.
fn borrow<'a, T>(ruby: &Ruby, cell: &'a RefCell<T>, what: &str) -> Result<RefMut<'a, T>, Error> {
    cell.try_borrow_mut().map_err(|_| {
        Error::new(
            ruby.exception_runtime_error(),
            format!("{what} is already in use (re-entrant call)"),
        )
    })
}

/// A row's styles by column: one Style for all, an Array, or none.
fn row_styles<'a>(
    ruby: &Ruby,
    style: Value,
    cells: usize,
) -> Result<Vec<Option<&'a Style>>, Error> {
    if style.is_nil() {
        return Ok(Vec::new());
    }
    let Some(styles) = RArray::from_value(style) else {
        return Ok(vec![Some(<&Style>::try_convert(style)?); cells]);
    };
    if styles.len() > cells {
        let message = format!("{} styles for a row of {cells} cells", styles.len());
        return Err(argument_error(ruby, message));
    }
    styles.into_iter().map(Option::<&Style>::try_convert).collect()
}

/// The symbols `align:` takes.
const ALIGNS: [(&str, Align); 3] =
    [("left", Align::Left), ("center", Align::Center), ("right", Align::Right)];
/// The symbols `direction:` takes.
const DIRECTIONS: [(&str, TextDirection); 2] =
    [("ltr", TextDirection::LeftToRight), ("rtl", TextDirection::RightToLeft)];

fn sheet_mut<'a>(
    ruby: &Ruby,
    book: &'a mut Workbook,
    index: usize,
) -> Result<&'a mut Worksheet, Error> {
    book.sheet_mut(index).ok_or_else(|| {
        Error::new(ruby.exception_index_error(), format!("no sheet at index {index}"))
    })
}

/// Imprimatur's errors as the Ruby exceptions they raise.
trait OrRaise<T> {
    fn or_raise(self, ruby: &Ruby) -> Result<T, Error>;
}

impl<T> OrRaise<T> for imprimatur::Result<T> {
    fn or_raise(self, ruby: &Ruby) -> Result<T, Error> {
        self.map_err(|error| domain_error(ruby, &error))
    }
}

/// An `ArgumentError` with this message.
fn argument_error(ruby: &Ruby, message: impl Into<std::borrow::Cow<'static, str>>) -> Error {
    Error::new(ruby.exception_arg_error(), message)
}

/// Raises `Imprimatur::Error`, or the subclass of it the error has, all
/// defined in `lib/imprimatur/error.rb`.
fn domain_error(ruby: &Ruby, error: &imprimatur::Error) -> Error {
    let name = match error {
        imprimatur::Error::NoTextLayer => "NoTextLayer",
        imprimatur::Error::PasswordRequired => "PasswordRequired",
        imprimatur::Error::UnsupportedDocument(_) => "UnsupportedDocument",
        imprimatur::Error::LimitExceeded(_) | imprimatur::Error::Cancelled => "LimitExceeded",
        _ => "Error",
    };
    let class = ruby
        .class_object()
        .const_get::<_, RModule>("Imprimatur")
        .and_then(|module| module.const_get::<_, ExceptionClass>(name));
    match class {
        Ok(class) => Error::new(class, error.to_string()),
        Err(lookup) => lookup,
    }
}

/// The value for a symbol among `choices`, or an `ArgumentError` naming them.
fn keyword<T: Copy>(
    ruby: &Ruby,
    name: &str,
    given: &str,
    choices: &[(&str, T)],
) -> Result<T, Error> {
    if let Some(&(_, value)) = choices.iter().find(|(symbol, _)| *symbol == given) {
        return Ok(value);
    }
    let expected: Vec<String> = choices.iter().map(|(symbol, _)| format!(":{symbol}")).collect();
    let message = format!("{name} must be one of {}, not :{given}", expected.join(", "));
    Err(argument_error(ruby, message))
}

#[magnus::init]
fn init(ruby: &Ruby) -> Result<(), Error> {
    let imprimatur = ruby.define_module("Imprimatur")?;
    let style = imprimatur.define_class("Style", ruby.class_object())?;
    style.define_singleton_method("__build", function!(Style::build, 13))?;
    let validation = imprimatur.define_class("Validation", ruby.class_object())?;
    validation.define_singleton_method("__build", function!(Validation::build, 8))?;

    let native = imprimatur.define_module("Native")?;
    native.define_singleton_method("markdown", function!(read::markdown_native, 4))?;
    native.define_singleton_method("content", function!(read::content_native, 4))?;
    native.define_singleton_method("images", function!(read::images_native, 4))?;
    document::define(ruby, native)?;
    let workbook = native.define_class("Workbook", ruby.class_object())?;
    workbook.define_singleton_method("new", function!(NativeWorkbook::new, 0))?;
    workbook.define_method("add_sheet", method!(NativeWorkbook::add_sheet, 1))?;
    workbook.define_method("append_row", method!(NativeWorkbook::append_row, 3))?;
    workbook.define_method("write", method!(NativeWorkbook::write, 3))?;
    workbook.define_method("set_column_width", method!(NativeWorkbook::set_column_width, 3))?;
    workbook.define_method("add_validation", method!(NativeWorkbook::add_validation, 3))?;
    workbook.define_method("set_comment", method!(NativeWorkbook::set_comment, 4))?;
    workbook.define_method("set_right_to_left", method!(NativeWorkbook::set_right_to_left, 2))?;
    workbook.define_method("next_row", method!(NativeWorkbook::next_row, 1))?;
    workbook.define_method("render", method!(NativeWorkbook::render, 0))?;
    workbook.define_method("save", method!(NativeWorkbook::save, 1))?;
    Ok(())
}
