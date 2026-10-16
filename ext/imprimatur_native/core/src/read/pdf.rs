//! PDF: the text layer as Markdown. Every page's content runs through a
//! small interpreter that places each glyph; layout turns the glyphs into
//! lines, paragraphs and headings. Scanned pages have no text layer:
//! they are images, and need OCR.

mod cmap;
mod content;
mod crypt;
mod document;
mod filter;
mod font;
mod glyphs;
mod images;
mod layout;
mod lexer;
mod object;
#[cfg(test)]
mod tests;
mod xref;

use super::Budget;
use super::content::Page;
use crate::{Error, Result};
use content::{Fonts, PageText};
use document::Document;
pub use images::images;
use xref::is_limit;

/// `%PDF-` within the first kilobyte, where readers look for it.
pub fn is_pdf(document: &[u8]) -> bool {
    document[..document.len().min(1024)].windows(5).any(|window| window == b"%PDF-")
}

pub fn content(data: &[u8], budget: &Budget<'_>) -> Result<Vec<Page>> {
    let document = Document::open(data, budget)?;
    let pages = document.pages()?;
    let mut fonts = Fonts::new();
    let mut lines = Vec::with_capacity(pages.len());
    let mut text = PageText::default();
    for page in &pages {
        budget.check()?;
        // A damaged page loses its text, not the document's.
        match content::page_text(&document, page, &mut fonts, &mut text) {
            Ok(()) => lines.push(layout::lines(&text)),
            Err(error) if is_limit(&error) => return Err(error),
            Err(_) => lines.push(layout::PageLines::default()),
        }
    }
    if lines.iter().all(|page| page.text.trim().is_empty()) {
        return Err(Error::NoTextLayer);
    }
    Ok(layout::pages(&lines))
}
