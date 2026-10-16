//! Cell comments (Excel's notes). The text lives in `xl/commentsN.xml`; the
//! box Excel draws for each one is a legacy VML shape in
//! `xl/drawings/vmlDrawingN.vml`, which Excel still requires to show them.

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, HashMap};

use super::cell::push_cell_reference;
use super::package::MAIN_NS;
use super::text::{check_length, escape, needs_preserve};
use super::worksheet::{MAX_COLUMNS, MAX_ROWS};
use crate::Result;
use crate::xml::XmlWriter;

/// VML shape ids come in blocks of 1024 per `o:idmap` entry.
pub const SHAPES_PER_BLOCK: u32 = 1024;
/// Excel's default note: 128 by 74 pixels, 15 to the right of the next
/// column and 10 below the row above, on a grid of 64 by 20 pixel cells.
const NOTE_SIZE: (u32, u32) = (128, 74);
const DEFAULT_COLUMN_PIXELS: u32 = 64;
const ROW_PIXELS: u32 = 20;
/// Calibri 11, the default font: its widest digit and the cell padding.
const DIGIT_PIXELS: f64 = 7.0;
const PADDING_PIXELS: f64 = 5.0;

/// A note on a cell, set with
/// [`Worksheet::set_comment`](super::Worksheet::set_comment). Hidden until
/// the pointer rests on the cell, unless made visible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    text: String,
    author: Option<String>,
    visible: bool,
}

impl Comment {
    /// # Errors
    /// [`Error::TextTooLong`](crate::Error::TextTooLong) over 32,767 characters.
    pub fn new(text: impl Into<String>) -> Result<Self> {
        let text = text.into();
        check_length(&text)?;
        Ok(Self { text, author: None, visible: false })
    }

    #[must_use]
    pub fn author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into()).filter(|author| !author.is_empty());
        self
    }

    /// Shows the note all the time, not only on hover.
    #[must_use]
    pub const fn visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// `xl/commentsN.xml`: the authors, then each comment in row-major order.
/// A comment without an author refers to an empty one.
pub fn comments_xml(comments: &BTreeMap<(u32, u16), Comment>) -> String {
    let mut authors: Vec<&str> = Vec::new();
    let mut author_ids: HashMap<&str, usize> = HashMap::new();
    let ids: Vec<usize> = comments
        .values()
        .map(|comment| {
            let author = comment.author.as_deref().unwrap_or("");
            *author_ids.entry(author).or_insert_with(|| {
                authors.push(author);
                authors.len() - 1
            })
        })
        .collect();

    let mut xml = XmlWriter::new();
    xml.open("comments", &[("xmlns", MAIN_NS)]);
    xml.open("authors", &[]);
    for author in &authors {
        xml.element("author", &[], &escape(author));
    }
    xml.close("authors");
    xml.open("commentList", &[]);
    let mut reference = String::new();
    for ((&(row, column), comment), author) in comments.iter().zip(ids) {
        reference.clear();
        push_cell_reference(&mut reference, row, column);
        xml.open("comment", &[("ref", &reference), ("authorId", &author.to_string())]);
        xml.open("text", &[]);
        let preserve: &[(&str, &str)] =
            if needs_preserve(&comment.text) { &[("xml:space", "preserve")] } else { &[] };
        xml.element("t", preserve, &escape(&comment.text));
        xml.close("text");
        xml.close("comment");
    }
    xml.close("commentList");
    xml.close("comments");
    xml.finish()
}

/// How many shape id blocks a drawing of `count` notes takes: ids run from
/// `block * 1024 + 1`, so the last of a full block spills into the next.
pub fn blocks_for(count: usize) -> u32 {
    u32::try_from(count).map_or(u32::MAX, |count| count / SHAPES_PER_BLOCK + 1)
}

/// `xl/drawings/vmlDrawingN.vml`: one note shape per comment, with ids
/// from `first_block`, which no other drawing in the workbook uses.
pub fn vml(
    comments: &BTreeMap<(u32, u16), Comment>,
    widths: &BTreeMap<u16, f64>,
    first_block: u32,
) -> String {
    let mut xml = XmlWriter::bare();
    xml.open(
        "xml",
        &[
            ("xmlns:v", "urn:schemas-microsoft-com:vml"),
            ("xmlns:o", "urn:schemas-microsoft-com:office:office"),
            ("xmlns:x", "urn:schemas-microsoft-com:office:excel"),
        ],
    );
    xml.open("o:shapelayout", &[("v:ext", "edit")]);
    let blocks: Vec<String> =
        (first_block..first_block + blocks_for(comments.len())).map(|b| b.to_string()).collect();
    xml.empty("o:idmap", &[("v:ext", "edit"), ("data", &blocks.join(","))]);
    xml.close("o:shapelayout");
    xml.open(
        "v:shapetype",
        &[
            ("id", "_x0000_t202"),
            ("coordsize", "21600,21600"),
            ("o:spt", "202"),
            ("path", "m,l,21600r21600,l21600,xe"),
        ],
    );
    xml.empty("v:stroke", &[("joinstyle", "miter")]);
    xml.empty("v:path", &[("gradientshapeok", "t"), ("o:connecttype", "rect")]);
    xml.close("v:shapetype");
    let first_id = first_block * SHAPES_PER_BLOCK + 1;
    for (index, (&(row, column), comment)) in (0..).zip(comments) {
        write_shape(&mut xml, first_id + index, index + 1, (row, column, comment.visible), widths);
    }
    xml.close("xml");
    xml.finish()
}

fn write_shape(
    xml: &mut XmlWriter,
    id: u32,
    z_index: u32,
    (row, column, visible): (u32, u16, bool),
    widths: &BTreeMap<u16, f64>,
) {
    let anchor = Anchor::new(row, column, widths);
    let points = |pixels: u32| f64::from(pixels) * 0.75;
    let style = format!(
        "position:absolute;margin-left:{}pt;margin-top:{}pt;width:{}pt;height:{}pt;\
         z-index:{z_index};visibility:{}",
        points(anchor.left),
        points(anchor.top),
        points(NOTE_SIZE.0),
        points(NOTE_SIZE.1),
        if visible { "visible" } else { "hidden" },
    );
    let shape_id = format!("_x0000_s{id}");
    xml.open(
        "v:shape",
        &[
            ("id", &shape_id),
            ("type", "#_x0000_t202"),
            ("style", &style),
            ("fillcolor", "#ffffe1"),
            ("o:insetmode", "auto"),
        ],
    );
    xml.empty("v:fill", &[("color2", "#ffffe1")]);
    xml.empty("v:shadow", &[("on", "t"), ("color", "black"), ("obscured", "t")]);
    xml.empty("v:path", &[("o:connecttype", "none")]);
    xml.open("v:textbox", &[("style", "mso-direction-alt:auto")]);
    xml.element("div", &[("style", "text-align:left")], "");
    xml.close("v:textbox");
    xml.open("x:ClientData", &[("ObjectType", "Note")]);
    xml.empty("x:MoveWithCells", &[]);
    xml.empty("x:SizeWithCells", &[]);
    xml.element("x:Anchor", &[], &anchor.cells);
    xml.element("x:AutoFill", &[], "False");
    xml.element("x:Row", &[], &row.to_string());
    xml.element("x:Column", &[], &column.to_string());
    if visible {
        xml.empty("x:Visible", &[]);
    }
    xml.close("x:ClientData");
    xml.close("v:shape");
}

/// Where a note's box sits: `x:Anchor` in cells and pixel offsets, and its
/// top-left corner in pixels from the sheet's.
struct Anchor {
    cells: String,
    left: u32,
    top: u32,
}

impl Anchor {
    /// Beside the cell, from the next column and the row above, as Excel
    /// places a new note; held inside the sheet at its right and bottom edges.
    fn new(row: u32, column: u16, widths: &BTreeMap<u16, f64>) -> Self {
        let width = |column: u16| widths.get(&column).map_or(DEFAULT_COLUMN_PIXELS, |&w| pixels(w));
        // Past the columns an offset spans, as Excel counts them: narrower
        // columns are skipped.
        let walk = |mut column: u16, mut offset: u32| {
            while offset >= width(column) && column < MAX_COLUMNS - 1 {
                offset -= width(column);
                column += 1;
            }
            (column, offset)
        };
        let (first_column, x1) = walk((column + 1).min(MAX_COLUMNS - 4), 15);
        let (last_column, x2) = walk(first_column, x1 + NOTE_SIZE.0);
        let (first_row, y1) =
            if row == 0 { (0, 2) } else { ((row - 1).min(MAX_ROWS - 6), ROW_PIXELS / 2) };
        let height = y1 + NOTE_SIZE.1;
        let (last_row, y2) = (first_row + height / ROW_PIXELS, height % ROW_PIXELS);

        let custom: i64 = widths
            .range(..first_column)
            .map(|(_, &w)| i64::from(pixels(w)) - i64::from(DEFAULT_COLUMN_PIXELS))
            .sum();
        let left = i64::from(first_column) * i64::from(DEFAULT_COLUMN_PIXELS) + custom;
        let left = u32::try_from(left).unwrap_or(0) + x1;
        let top = first_row.saturating_mul(ROW_PIXELS).saturating_add(y1);
        let cells = format!(
            "{first_column}, {x1}, {first_row}, {y1}, {last_column}, {x2}, {last_row}, {y2}"
        );
        Self { cells, left, top }
    }
}

/// A column width in characters as Excel draws it, in pixels.
#[expect(clippy::cast_possible_truncation, reason = "widths are 0 to 255: at most 1791")]
#[expect(clippy::cast_sign_loss, reason = "widths are never negative")]
fn pixels(width: f64) -> u32 {
    let pixels = if width < 1.0 {
        width.mul_add(DIGIT_PIXELS + PADDING_PIXELS, 0.5)
    } else {
        width.mul_add(DIGIT_PIXELS, 0.5) + PADDING_PIXELS
    };
    pixels.floor() as u32
}
