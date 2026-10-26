//! PDF inspection, bounded text reads and optional full-page rendering.
//!
//! Every page's content runs through a
//! small interpreter that places each glyph; layout turns the glyphs into
//! lines, paragraphs and headings. Scanned pages have no text layer:
//! they are images, and need OCR.

mod bounded;
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

pub use bounded::{Info, PageResult, Pages, ReadOptions, Status, info, pages};
#[cfg(feature = "render")]
mod render;
#[cfg(feature = "render")]
pub use render::{RenderOptions, render_page};
#[cfg(all(feature = "isolation", unix))]
pub mod isolation;

use super::Budget;
use super::content::Page;
use crate::Result;
use content::{Fonts, PageText};
use document::Document;
pub(super) use images::images;
use xref::is_limit;

/// `%PDF-` within the first kilobyte, where readers look for it.
pub(super) fn is_pdf(document: &[u8]) -> bool {
    document[..document.len().min(1024)].windows(5).any(|window| window == b"%PDF-")
}

pub(super) fn content(data: &[u8], budget: &Budget<'_>) -> Result<Vec<Page>> {
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
    Ok(layout::pages(&lines, budget.drop_repeated()))
}
