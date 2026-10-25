//! Places the document on pages: one content stream per page.

#[cfg(test)]
mod tests;
mod wrap;

use std::fmt::{self, Arguments, Write as _};

use super::font::Face;
use super::objects::{Num, push_string};
use crate::doc::{Document, Paragraph, Picture, Symbol, Table};
use crate::style::{Align, Color};
use crate::{Error, Result};
use wrap::{Line, Look, wrap};

const TABLE_SIZE: f64 = 10.0;
/// Line height as a multiple of the font size.
const LEADING: f64 = 1.25;
const PARAGRAPH_GAP: f64 = 6.0;
const CELL_PADDING: f64 = 4.0;
const HEADING_COLOR: Color = Color::rgb(0x001F_3864);
const HEADER_FILL: Color = Color::rgb(0x00D9_E2F3);
const FOOTER_SIZE: f64 = 9.0;
const FOOTER_COLOR: Color = Color::rgb(0x0066_6666);
/// Size, space before, color of headings 1 to 3.
const HEADINGS: [(f64, f64, Option<Color>); 3] =
    [(20.0, 14.0, Some(HEADING_COLOR)), (15.0, 10.0, Some(HEADING_COLOR)), (12.5, 8.0, None)];

pub struct Layout {
    width: f64,
    height: f64,
    margin: f64,
    body_size: f64,
    pages: Vec<Vec<u8>>,
    /// The top of the free space on the current page, from the bottom edge.
    y: f64,
    blank: bool,
}

impl Layout {
    /// The document's pages, between its margins.
    ///
    /// # Errors
    /// [`Error::InvalidMargin`] when the margins leave no room.
    pub fn new(document: &Document) -> Result<Self> {
        document.room()?;
        let (width, height) = document.page_size().points();
        let margin = document.margin();
        Ok(Self {
            width,
            height,
            margin,
            body_size: document.font_size(),
            pages: vec![Vec::new()],
            y: height - margin,
            blank: true,
        })
    }

    pub const fn page_size(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    const fn content_width(&self) -> f64 {
        (-2.0f64).mul_add(self.margin, self.width)
    }

    pub fn heading(&mut self, level: u8, text: &str) -> Result<()> {
        let index = usize::from(level.clamp(1, 3) - 1);
        let (size, before, color) = HEADINGS[index];
        let look = Look { face: Face::Bold, color, underline: false };
        let lines = wrap(&[(Face::Bold.encode(text)?, look)], size, self.content_width());
        // Keep the heading with at least two lines of what follows.
        let body_lines = 2.0 * self.body_size * LEADING;
        let needed = (line_count(&lines) * size).mul_add(LEADING, before) + body_lines;
        self.ensure_room(needed);
        if !self.blank {
            self.y -= before;
        }
        self.lines(&lines, size, Align::Left);
        self.y -= PARAGRAPH_GAP;
        Ok(())
    }

    pub fn paragraph(&mut self, paragraph: &Paragraph) -> Result<()> {
        let mut runs = Vec::with_capacity(paragraph.runs().len());
        for run in paragraph.runs() {
            let face = Face::new(run.bold, run.italic);
            runs.push((
                face.encode(&run.text)?,
                Look { face, color: run.color, underline: run.underline },
            ));
        }
        let lines = wrap(&runs, self.body_size, self.content_width());
        self.lines(&lines, self.body_size, paragraph.effective_align());
        self.y -= PARAGRAPH_GAP;
        Ok(())
    }

    pub fn table(&mut self, table: &Table) -> Result<()> {
        let columns = table.columns();
        #[expect(clippy::cast_precision_loss, reason = "a column count is small")]
        let column_width = self.content_width() / columns as f64;
        let mut rows = Vec::with_capacity(table.rows().len());
        for (index, row) in table.rows().iter().enumerate() {
            let header = index == 0 && table.has_header();
            let face = if header { Face::Bold } else { Face::Regular };
            let look = Look { face, color: None, underline: false };
            let mut cells = Vec::with_capacity(columns);
            for column in 0..columns {
                let text = row.get(column).map_or("", String::as_str);
                let runs = [(face.encode(text)?, look)];
                cells.push(wrap(&runs, TABLE_SIZE, (-2.0f64).mul_add(CELL_PADDING, column_width)));
            }
            let tallest = cells.iter().map(|lines| line_count(lines)).fold(1.0, f64::max);
            let height = (tallest * TABLE_SIZE).mul_add(LEADING, 2.0 * CELL_PADDING);
            rows.push((cells, height, header));
        }

        let header_height = rows.first().filter(|row| row.2).map_or(0.0, |row| row.1);
        let room = (-2.0f64).mul_add(self.margin, self.height) - header_height;
        for (index, (cells, height, header)) in rows.iter().enumerate() {
            (*height <= room || *header).ok_or(Error::RowTooTall(*height))?;
            // A header needs the first body row beside it.
            let needed =
                if *header { height + rows.get(1).map_or(0.0, |row| row.1) } else { *height };
            if self.ensure_room(needed) && table.has_header() && index > 0 {
                let (cells, height, _) = &rows[0];
                self.row(cells, *height, column_width, true);
            }
            self.row(cells, *height, column_width, *header);
        }
        self.y -= PARAGRAPH_GAP;
        Ok(())
    }

    /// Draws picture `index` (`/Im{index}`), on a new page when it does not
    /// fit, as its alignment says.
    pub fn picture(&mut self, picture: &Picture, index: usize) {
        let (width, height) = picture.size(self.room());
        let (x, y) = self.place((width, height), picture.align);
        let (width, height, x, y) = (Num(width), Num(height), Num(x), Num(y));
        emit(self.current(), format_args!("q {width} 0 0 {height} {x} {y} cm /Im{index} Do Q\n"));
        self.end_figure();
    }

    /// Draws a barcode as filled rectangles, one module a unit, the
    /// coordinates flipped to run down from its top left corner.
    pub fn symbol(&mut self, symbol: &Symbol) {
        let (shapes, (width, height)) = symbol.placed(self.room());
        let (x, y) = self.place((width, height), symbol.align);
        let (columns, rows) = shapes.extent();
        let (across, down) = (width / columns, height / rows);
        let (fill, top, x) = (Fill(symbol.color), Num(y + height), Num(x));
        let page = self.current();
        // Four decimals: a module's size in hundredths of a point would be
        // off by up to half a percent across the symbol.
        emit(page, format_args!("q {fill} {across:.4} 0 0 {:.4} {x} {top} cm\n", -down));
        for [x, y, run, rows] in &shapes.rects {
            emit(page, format_args!("{x} {y} {run} {rows} re\n"));
        }
        page.extend_from_slice(b"f Q\n");
        self.end_figure();
    }

    pub fn page_break(&mut self) {
        self.new_page();
    }

    /// The finished pages, numbered `1 / N` at the bottom when asked.
    pub fn finish(mut self, page_numbers: bool) -> Vec<Vec<u8>> {
        if page_numbers {
            let total = self.pages.len();
            let (center, margin) = (self.width / 2.0, self.margin);
            for (index, page) in self.pages.iter_mut().enumerate() {
                let bytes = format!("{} / {total}", index + 1).into_bytes();
                let width = Face::Regular.width(&bytes, FOOTER_SIZE);
                let look =
                    Look { face: Face::Regular, color: Some(FOOTER_COLOR), underline: false };
                let label = wrap::Fragment { bytes, look, width };
                draw_fragment(page, &label, FOOTER_SIZE, center - width / 2.0, margin / 2.0);
            }
        }
        self.pages
    }

    /// The width and height between the margins.
    const fn room(&self) -> (f64, f64) {
        (self.content_width(), (-2.0f64).mul_add(self.margin, self.height))
    }

    /// Makes room for a figure of `size`, on a new page when it does not
    /// fit, and returns its bottom left corner as `align` places it.
    fn place(&mut self, (width, height): (f64, f64), align: Option<Align>) -> (f64, f64) {
        self.ensure_room(height);
        let x = self.margin
            + match align.unwrap_or(Align::Left) {
                Align::Left => 0.0,
                Align::Center => (self.content_width() - width) / 2.0,
                Align::Right => self.content_width() - width,
            };
        self.y -= height;
        (x, self.y)
    }

    fn end_figure(&mut self) {
        self.y -= PARAGRAPH_GAP;
        self.blank = false;
    }

    /// Starts a new page unless `needed` points fit above the bottom margin
    /// (or the page is still blank, where nothing would fit better).
    /// Returns whether it did.
    fn ensure_room(&mut self, needed: f64) -> bool {
        let full = !self.blank && self.y - needed < self.margin;
        if full {
            self.new_page();
        }
        full
    }

    fn new_page(&mut self) {
        self.pages.push(Vec::new());
        self.y = self.height - self.margin;
        self.blank = true;
    }

    fn current(&mut self) -> &mut Vec<u8> {
        if self.pages.is_empty() {
            self.pages.push(Vec::new());
        }
        let last = self.pages.len() - 1;
        &mut self.pages[last]
    }

    /// Draws lines from the cursor down, starting new pages as needed.
    fn lines(&mut self, lines: &[Line], size: f64, align: Align) {
        let leading = size * LEADING;
        for line in lines {
            self.ensure_room(leading);
            let baseline = self.y - baseline_offset(size);
            let slack = self.content_width() - line.width;
            let x = self.margin
                + match align {
                    Align::Left => 0.0,
                    Align::Center => slack / 2.0,
                    Align::Right => slack,
                };
            draw_line(self.current(), line, size, x, baseline);
            self.y -= leading;
            self.blank = false;
        }
    }

    /// One table row from the cursor down: fill, borders, then text.
    fn row(&mut self, cells: &[Vec<Line>], height: f64, column_width: f64, header: bool) {
        let (top, mut x) = (self.y, self.margin);
        let page = self.current();
        for lines in cells {
            let rect = format_args!(
                "{} {} {} {} re",
                Num(x),
                Num(top - height),
                Num(column_width),
                Num(height)
            );
            if header {
                emit(page, format_args!("{} {rect} f\n", Fill(HEADER_FILL)));
            }
            emit(page, format_args!("0.5 w 0 0 0 RG {rect} S\n"));
            let mut y = top - CELL_PADDING;
            for line in lines {
                let baseline = y - baseline_offset(TABLE_SIZE);
                draw_line(page, line, TABLE_SIZE, x + CELL_PADDING, baseline);
                y = TABLE_SIZE.mul_add(-LEADING, y);
            }
            x += column_width;
        }
        self.y -= height;
        self.blank = false;
    }
}

#[expect(clippy::cast_precision_loss, reason = "a paragraph has far fewer than 2**53 lines")]
const fn line_count(lines: &[Line]) -> f64 {
    lines.len() as f64
}

/// From the top of a line to its baseline: half the leading the glyphs do
/// not fill, then the ascender.
fn baseline_offset(size: f64) -> f64 {
    let (ascent, descent) = (Face::Regular.ascent(size), Face::Regular.descent(size));
    size.mul_add(LEADING, -ascent - descent).mul_add(0.5, ascent)
}

/// Draws a line's fragments left to right from `x`.
fn draw_line(page: &mut Vec<u8>, line: &Line, size: f64, mut x: f64, baseline: f64) {
    for fragment in &line.fragments {
        draw_fragment(page, fragment, size, x, baseline);
        x += fragment.width;
    }
}

fn draw_fragment(page: &mut Vec<u8>, fragment: &wrap::Fragment, size: f64, x: f64, baseline: f64) {
    let look = fragment.look;
    let color = Fill(look.color.unwrap_or(Color::BLACK));
    let (face, size_num) = (look.face.resource(), Num(size));
    emit(page, format_args!("BT /{face} {size_num} Tf {color} {} {} Td ", Num(x), Num(baseline)));
    push_string(page, &fragment.bytes);
    page.extend_from_slice(b" Tj ET\n");
    if look.underline && fragment.width > 0.0 {
        // Helvetica's underline: 100 below the baseline, 50 thick.
        let (y, thickness) = ((-0.125f64).mul_add(size, baseline), 0.05 * size);
        let (x, y, width, thickness) = (Num(x), Num(y), Num(fragment.width), Num(thickness));
        emit(page, format_args!("{color} {x} {y} {width} {thickness} re f\n"));
    }
}

/// `r g b rg`, the fill color operator.
#[derive(Clone, Copy)]
struct Fill(Color);

impl fmt::Display for Fill {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] = self.0.components();
        write!(f, "{} {} {} rg", Num(r), Num(g), Num(b))
    }
}

/// Appends operators to a content stream, formatted in place.
fn emit(page: &mut Vec<u8>, ops: Arguments<'_>) {
    // `Ops` never fails and every value formatted here is a number, a name
    // or a `Fill`, so formatting cannot fail either.
    let _ = Ops(page).write_fmt(ops);
}

/// `fmt::Write` over a content stream.
struct Ops<'a>(&'a mut Vec<u8>);

impl fmt::Write for Ops<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.0.extend_from_slice(text.as_bytes());
        Ok(())
    }
}
