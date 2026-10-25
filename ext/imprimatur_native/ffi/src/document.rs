//! `Imprimatur::Native::Document`: the document model, rendered to PDF or DOCX.

use std::cell::RefCell;

use imprimatur::doc::{Document, PageSize, Paragraph, Picture, Run, Symbol, Table};
use imprimatur::style::Color;
use magnus::{Error, RModule, RString, Ruby, function, method, prelude::*};

use crate::barcode::Barcode;
use crate::{ALIGNS, DIRECTIONS, OrRaise, RubyBytes, borrow, keyword};

/// `[text, bold, italic, underline, color]`, as `Imprimatur::Paragraph` collects it.
type RunSpec = (String, bool, bool, bool, Option<String>);

#[magnus::wrap(class = "Imprimatur::Native::Document", free_immediately, size)]
struct NativeDocument(RefCell<Document>);

#[derive(Clone, Copy)]
enum Output {
    Pdf,
    Docx,
}

const OUTPUTS: [(&str, Output); 2] = [("pdf", Output::Pdf), ("docx", Output::Docx)];

#[expect(clippy::needless_pass_by_value, reason = "magnus passes Ruby arguments as owned values")]
impl NativeDocument {
    fn new() -> Self {
        Self(RefCell::new(Document::new()))
    }

    fn set_page(
        ruby: &Ruby,
        rb_self: &Self,
        size: String,
        page_numbers: bool,
    ) -> Result<(), Error> {
        let size = keyword(
            ruby,
            "page_size",
            &size,
            &[("a4", PageSize::A4), ("letter", PageSize::Letter)],
        )?;
        Self::page(ruby, rb_self, size, page_numbers)
    }

    /// A page `width` by `height` points.
    fn set_page_points(
        ruby: &Ruby,
        rb_self: &Self,
        width: f64,
        height: f64,
        page_numbers: bool,
    ) -> Result<(), Error> {
        let size = PageSize::custom(width, height).or_raise(ruby)?;
        Self::page(ruby, rb_self, size, page_numbers)
    }

    fn page(ruby: &Ruby, rb_self: &Self, size: PageSize, page_numbers: bool) -> Result<(), Error> {
        borrow(ruby, &rb_self.0, "document")?.set_page_size(size).set_page_numbers(page_numbers);
        Ok(())
    }

    fn set_margin(ruby: &Ruby, rb_self: &Self, points: f64) -> Result<(), Error> {
        borrow(ruby, &rb_self.0, "document")?.set_margin(points).or_raise(ruby)?;
        Ok(())
    }

    fn set_font_size(ruby: &Ruby, rb_self: &Self, points: f64) -> Result<(), Error> {
        borrow(ruby, &rb_self.0, "document")?.set_font_size(points).or_raise(ruby)?;
        Ok(())
    }

    /// A PNG or JPEG from its bytes, `width` and `height` in points.
    fn picture(
        ruby: &Ruby,
        rb_self: &Self,
        data: RString,
        width: Option<f64>,
        height: Option<f64>,
        align: Option<String>,
    ) -> Result<(), Error> {
        let mut picture = Picture::new(crate::bytes(data)).or_raise(ruby)?;
        if let Some(width) = width {
            picture = picture.width(width).or_raise(ruby)?;
        }
        if let Some(height) = height {
            picture = picture.height(height).or_raise(ruby)?;
        }
        if let Some(align) = align {
            picture = picture.align(keyword(ruby, "align", &align, &ALIGNS)?);
        }
        borrow(ruby, &rb_self.0, "document")?.picture(picture);
        Ok(())
    }

    /// A barcode, `width` and `height` in points, in a `color` "#RRGGBB".
    fn barcode(
        ruby: &Ruby,
        rb_self: &Self,
        barcode: &Barcode,
        (width, height): (Option<f64>, Option<f64>),
        align: Option<String>,
        color: Option<String>,
    ) -> Result<(), Error> {
        let mut symbol = Symbol::new(barcode.0.clone());
        if let Some(width) = width {
            symbol = symbol.width(width).or_raise(ruby)?;
        }
        if let Some(height) = height {
            symbol = symbol.height(height).or_raise(ruby)?;
        }
        if let Some(align) = align {
            symbol = symbol.align(keyword(ruby, "align", &align, &ALIGNS)?);
        }
        if let Some(color) = color {
            symbol = symbol.color(Color::from_hex(&color).or_raise(ruby)?);
        }
        borrow(ruby, &rb_self.0, "document")?.barcode(symbol);
        Ok(())
    }

    fn heading(ruby: &Ruby, rb_self: &Self, level: u8, text: String) -> Result<(), Error> {
        borrow(ruby, &rb_self.0, "document")?.heading(level, text).or_raise(ruby)?;
        Ok(())
    }

    fn paragraph(
        ruby: &Ruby,
        rb_self: &Self,
        runs: Vec<RunSpec>,
        align: Option<String>,
        direction: Option<String>,
    ) -> Result<(), Error> {
        let mut paragraph = Paragraph::new();
        for (text, bold, italic, underline, color) in runs {
            let mut run = Run::new(text);
            if bold {
                run = run.bold();
            }
            if italic {
                run = run.italic();
            }
            if underline {
                run = run.underline();
            }
            if let Some(color) = color {
                run = run.color(Color::from_hex(&color).or_raise(ruby)?);
            }
            paragraph = paragraph.run(run);
        }
        if let Some(align) = align {
            paragraph = paragraph.align(keyword(ruby, "align", &align, &ALIGNS)?);
        }
        if let Some(direction) = direction {
            paragraph = paragraph.direction(keyword(ruby, "direction", &direction, &DIRECTIONS)?);
        }
        borrow(ruby, &rb_self.0, "document")?.push_paragraph(paragraph).or_raise(ruby)?;
        Ok(())
    }

    fn table(
        ruby: &Ruby,
        rb_self: &Self,
        rows: Vec<Vec<String>>,
        header: bool,
    ) -> Result<(), Error> {
        let mut table = Table::new(rows);
        if header {
            table = table.header();
        }
        borrow(ruby, &rb_self.0, "document")?.table(table).or_raise(ruby)?;
        Ok(())
    }

    fn page_break(ruby: &Ruby, rb_self: &Self) -> Result<(), Error> {
        borrow(ruby, &rb_self.0, "document")?.page_break();
        Ok(())
    }

    /// `:pdf` or `:docx` as a binary String.
    fn render(ruby: &Ruby, rb_self: &Self, format: String) -> Result<RString, Error> {
        let document = borrow(ruby, &rb_self.0, "document")?;
        let out = RubyBytes(ruby.str_buf_new(0));
        let written = match keyword(ruby, "format", &format, &OUTPUTS)? {
            Output::Pdf => document.write_pdf(out),
            Output::Docx => document.write_docx(out),
        };
        Ok(written.or_raise(ruby)?.0)
    }

    /// Writes `:pdf` or `:docx` straight to `path`.
    fn save(ruby: &Ruby, rb_self: &Self, format: String, path: String) -> Result<(), Error> {
        let document = borrow(ruby, &rb_self.0, "document")?;
        let saved = match keyword(ruby, "format", &format, &OUTPUTS)? {
            Output::Pdf => document.save_pdf(path),
            Output::Docx => document.save_docx(path),
        };
        saved.or_raise(ruby)
    }
}

pub fn define(ruby: &Ruby, native: RModule) -> Result<(), Error> {
    let class = native.define_class("Document", ruby.class_object())?;
    class.define_singleton_method("new", function!(NativeDocument::new, 0))?;
    class.define_method("set_page", method!(NativeDocument::set_page, 2))?;
    class.define_method("set_page_points", method!(NativeDocument::set_page_points, 3))?;
    class.define_method("set_margin", method!(NativeDocument::set_margin, 1))?;
    class.define_method("set_font_size", method!(NativeDocument::set_font_size, 1))?;
    class.define_method("picture", method!(NativeDocument::picture, 4))?;
    class.define_method("barcode", method!(NativeDocument::barcode, 4))?;
    class.define_method("heading", method!(NativeDocument::heading, 2))?;
    class.define_method("paragraph", method!(NativeDocument::paragraph, 3))?;
    class.define_method("table", method!(NativeDocument::table, 2))?;
    class.define_method("page_break", method!(NativeDocument::page_break, 0))?;
    class.define_method("render", method!(NativeDocument::render, 1))?;
    class.define_method("save", method!(NativeDocument::save, 2))?;
    Ok(())
}
