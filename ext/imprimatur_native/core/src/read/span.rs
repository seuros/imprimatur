//! Text kept in one buffer and referred to by spans: a page's glyphs and
//! lines, a font's encoding, a `CMap`'s targets. A `u32` span is half a
//! `Range<usize>`, and one buffer is one allocation instead of many.

use std::ops::Range;

/// The end of `text`, as a span bound.
pub fn end(text: &str) -> u32 {
    u32::try_from(text.len()).unwrap_or(u32::MAX)
}

/// Appends what `push` writes to `text`; returns its span.
pub fn append(text: &mut String, push: impl FnOnce(&mut String)) -> Range<u32> {
    let start = end(text);
    push(text);
    start..end(text)
}

/// The text of a span.
pub fn slice<'t>(text: &'t str, span: &Range<u32>) -> &'t str {
    &text[span.start as usize..span.end as usize]
}
