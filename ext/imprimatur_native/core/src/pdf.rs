//! PDF output: the document laid out with the standard Helvetica fonts
//! (`WinAnsiEncoding`, so Windows-1252 text), written with our own object
//! writer. `truetype` serializes embedded font resources; connecting these to
//! document layout, shaping and bidirectional text comes next.

mod font;
mod layout;
#[rustfmt::skip]
pub mod metrics;
mod objects;
mod picture;
#[cfg(test)]
mod tests;
#[cfg(feature = "fonts")]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "font resources are connected to document layout in a follow-up")
)]
mod truetype;

use std::fmt;
use std::io::Write;

use crate::Result;
use crate::doc::{Block, Document};
use font::Face;
use layout::Layout;
use objects::{Num, PdfWriter};

const CATALOG: usize = 1;
const PAGES: usize = 2;
/// Fonts are objects 3 to 6, in [`Face::ALL`] order; the info dictionary follows.
const FIRST_FONT: usize = 3;
const INFO: usize = FIRST_FONT + Face::ALL.len();

pub fn write<W: Write>(document: &Document, out: W) -> Result<W> {
    let mut layout = Layout::new(document)?;
    let mut pictures = Vec::new();
    for block in document.blocks() {
        match block {
            Block::Heading { level, text } => layout.heading(*level, text)?,
            Block::Paragraph(paragraph) => layout.paragraph(paragraph)?,
            Block::Table(table) => layout.table(table)?,
            Block::Picture(picture) => {
                layout.picture(picture, pictures.len());
                pictures.push(picture);
            }
            Block::PageBreak => layout.page_break(),
        }
    }
    let (width, height) = layout.page_size();
    let pages = layout.finish(document.has_page_numbers());

    let mut pdf = PdfWriter::new(out)?;
    // Each page is a page object followed by its content stream.
    let page_id = |index: usize| INFO + 1 + 2 * index;
    pdf.object(CATALOG, format_args!("<< /Type /Catalog /Pages {PAGES} 0 R >>"))?;
    let kids = fmt::from_fn(|f| {
        for index in 0..pages.len() {
            write!(f, "{} 0 R ", page_id(index))?;
        }
        Ok(())
    });
    let count = pages.len();
    pdf.object(PAGES, format_args!("<< /Type /Pages /Kids [{kids}] /Count {count} >>"))?;
    for (id, face) in (FIRST_FONT..).zip(Face::ALL) {
        let name = face.metrics().name;
        pdf.object(
            id,
            format_args!(
                "<< /Type /Font /Subtype /Type1 /BaseFont /{name} /Encoding /WinAnsiEncoding >>"
            ),
        )?;
    }
    pdf.object(INFO, format_args!("<< /Producer (imprimatur) >>"))?;

    let fonts = fmt::from_fn(|f| {
        for (id, face) in (FIRST_FONT..).zip(Face::ALL) {
            write!(f, "/{} {id} 0 R ", face.resource())?;
        }
        Ok(())
    });
    // Pictures follow the pages: an image, then its soft mask if it has one.
    let mut ids = Vec::with_capacity(pictures.len());
    let mut next = page_id(pages.len());
    for picture in &pictures {
        ids.push(next);
        next += 1 + usize::from(picture::has_mask(picture));
    }
    let images = fmt::from_fn(|f| {
        if !ids.is_empty() {
            f.write_str("/XObject << ")?;
            for (index, id) in ids.iter().enumerate() {
                write!(f, "/Im{index} {id} 0 R ")?;
            }
            f.write_str(">> ")?;
        }
        Ok(())
    });
    let (width, height) = (Num(width), Num(height));
    for (index, content) in pages.iter().enumerate() {
        let id = page_id(index);
        pdf.object(
            id,
            format_args!(
                "<< /Type /Page /Parent {PAGES} 0 R /MediaBox [0 0 {width} {height}] \
                 /Resources << /Font << {fonts}>> {images}>> /Contents {} 0 R >>",
                id + 1
            ),
        )?;
        pdf.stream(id + 1, content, &[])?;
    }
    for (picture, id) in pictures.into_iter().zip(ids) {
        for (offset, stream) in (0..).zip(picture::streams(picture, id + 1)?) {
            pdf.stored(id + offset, format_args!("{}", stream.dictionary), &stream.data)?;
        }
    }
    pdf.finish(CATALOG, INFO)
}
