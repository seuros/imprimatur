//! From glyphs to blocks: lines by baseline, words by the gaps between
//! glyphs, paragraphs by the gaps between lines, headings by size, and
//! the running headers and footers left out.

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ops::Range;

use super::content::{Glyph, PageText};
use crate::read::content::{Block, Bounds, Page, Placed, Text};
use crate::read::span;

/// Share of the page height, at the top and the bottom, where running
/// headers and footers live.
const EDGE: f64 = 0.09;
/// A gap wider than this share of the font size separates words.
const WORD_GAP: f64 = 0.18;

/// A line of a page, its text a span of the page's buffer.
#[derive(Debug, Clone)]
pub struct Line {
    pub span: Range<u32>,
    /// The baseline, in the frame where this line's text runs left to right.
    pub y: f64,
    /// Where the line starts and ends along its baseline, in that frame.
    pub left: f64,
    pub right: f64,
    pub size: f64,
    pub bold: bool,
    pub turns: u8,
    /// In the top or bottom margin of an upright page: a hash of the text,
    /// digits aside, to spot running headers and footers. 0 elsewhere.
    pub margin: u64,
}

/// A page's lines and their text (one buffer for all of them), its height
/// as shown, and how many images it draws.
#[derive(Debug, Default)]
pub struct PageLines {
    pub text: String,
    pub lines: Vec<Line>,
    pub height: f64,
    pub images: u32,
}

impl PageLines {
    pub fn text(&self, line: &Line) -> &str {
        span::slice(&self.text, &line.span)
    }
}

/// A page's lines: upright text first, top to bottom, then each other
/// direction in turn.
pub fn lines(page: &PageText) -> PageLines {
    let height = page.size.1.max(1.0);
    let mut out = PageLines {
        text: String::with_capacity(page.text.len() + page.text.len() / 8),
        lines: Vec::new(),
        height,
        images: u32::try_from(page.images).unwrap_or(u32::MAX),
    };
    let mut pieces: Vec<&str> = Vec::new();
    let mut glyphs: Vec<(f64, f64, &Glyph)> = Vec::with_capacity(page.glyphs.len());
    for turns in 0..4_u8 {
        glyphs.clear();
        glyphs.extend(page.glyphs.iter().filter(|glyph| glyph.turns == turns).map(|glyph| {
            let (x, y) = match turns {
                1 => (glyph.y, -glyph.x),
                2 => (-glyph.x, -glyph.y),
                3 => (-glyph.y, glyph.x),
                _ => (glyph.x, glyph.y),
            };
            (x, y, glyph)
        }));
        glyphs.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
        let mut start = 0;
        while start < glyphs.len() {
            let (_, baseline, first) = glyphs[start];
            let mut end = start + 1;
            while end < glyphs.len() {
                let (_, y, glyph) = glyphs[end];
                if (baseline - y).abs() > 0.45 * first.size.max(glyph.size) {
                    break;
                }
                end += 1;
            }
            let row = &mut glyphs[start..end];
            row.sort_by(|a, b| a.0.total_cmp(&b.0));
            let added = out.lines.len();
            line(page, row, turns, &mut pieces, &mut out);
            if let Some(line) = out.lines.get_mut(added)
                && turns == 0
                && (line.y > height * (1.0 - EDGE) || line.y < height * EDGE)
            {
                let text = span::slice(&out.text, &line.span);
                line.margin = margin_key(text);
            }
            start = end;
        }
    }
    out
}

/// One line's text, appended to `out.text`: spaces where glyphs leave a
/// word gap, a glyph drawn twice on itself (fake bold) kept once, and
/// right-to-left lines in reading order. A line with no text is not added.
fn line<'p>(
    page: &'p PageText,
    row: &[(f64, f64, &Glyph)],
    turns: u8,
    pieces: &mut Vec<&'p str>,
    out: &mut PageLines,
) {
    // Glyph texts and the spaces between them, left to right as drawn.
    pieces.clear();
    let mut previous: Option<(f64, &Glyph, &str)> = None;
    let (mut size, mut bold, mut count) = (0.0_f64, 0_usize, 0_usize);
    let mut right = f64::MIN;
    for &(x, _, glyph) in row {
        let glyph_text = &page.text[glyph.text.start as usize..glyph.text.end as usize];
        if let Some((previous_x, last, last_text)) = previous {
            if last_text == glyph_text && (x - previous_x).abs() < 0.2 * glyph.size.max(1.0) {
                continue;
            }
            let gap = x - (previous_x + last.width);
            let blank = glyph_text.starts_with(' ')
                || last_text.ends_with(' ')
                || glyph_text.starts_with(is_mark);
            if !blank && gap > WORD_GAP * glyph.size.min(last.size).max(1.0) {
                pieces.push(" ");
            }
        }
        pieces.push(glyph_text);
        right = right.max(x + glyph.width);
        size = size.max(glyph.size);
        bold += usize::from(glyph.bold);
        count += 1;
        previous = Some((x, glyph, glyph_text));
    }
    if right_to_left(pieces) {
        reorder(pieces);
    }
    // Spaces collapsed and trimmed on the way in.
    let start = out.text.len();
    for ch in pieces.iter().flat_map(|piece| piece.chars()) {
        if ch == ' ' && (out.text.len() == start || out.text.ends_with(' ')) {
            continue;
        }
        out.text.push(ch);
    }
    if out.text.ends_with(' ') {
        out.text.pop();
    }
    if out.text.len() == start {
        return;
    }
    let (left, y, _) = row[0];
    let span = span::end(&out.text[..start])..span::end(&out.text);
    let bold = bold * 2 > count;
    out.lines.push(Line { span, y, left, right, size, bold, turns, margin: 0 });
}

/// A hash of a line's text with digits as `#` and ASCII letters lowercased
/// (FNV-1a), never 0: page 3 of 9 and page 4 of 9 are one running line.
fn margin_key(text: &str) -> u64 {
    let hash = text.chars().fold(0xcbf2_9ce4_8422_2325_u64, |hash, ch| {
        let ch = if ch.is_ascii_digit() { '#' } else { ch.to_ascii_lowercase() };
        (hash ^ u64::from(u32::from(ch))).wrapping_mul(0x0100_0000_01b3)
    });
    hash.max(1)
}

/// The strong direction of a character: `Some(true)` for the scripts
/// written right to left, `Some(false)` for other letters and for digits
/// (which keep their order within right-to-left text), `None` for spaces
/// and punctuation.
///
/// Right to left: Hebrew (and Yiddish, Ladino), Arabic (and Persian, Urdu,
/// Pashto, Sindhi, Kurdish, Uyghur), Syriac, Thaana (Dhivehi), N'Ko,
/// Samaritan, Mandaic, their presentation forms; Adlam (Fulani), Hanifi
/// Rohingya, Mende Kikakui, Yezidi, Garay and the historic scripts
/// (Phoenician, Aramaic, Sogdian...); and the right-to-left mark.
fn direction(ch: char) -> Option<bool> {
    match ch {
        '\u{0590}'..='\u{08FF}'
        | '\u{200F}'
        | '\u{FB1D}'..='\u{FDFF}'
        | '\u{FE70}'..='\u{FEFC}'
        | '\u{10800}'..='\u{10FFF}'
        | '\u{1E800}'..='\u{1EFFF}' => Some(true),
        '\u{200E}' => Some(false),
        ch if ch.is_alphanumeric() => Some(false),
        _ => None,
    }
}

/// Combining marks: diacritics, and the vowel points and signs of the
/// right-to-left scripts. Drawn as glyphs of their own, they never start a
/// word.
const fn is_mark(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}'
            // Hebrew points and cantillation.
            | '\u{0591}'..='\u{05C7}'
            | '\u{FB1E}'
            // Arabic harakat, Quranic annotation, extended Arabic marks.
            | '\u{0610}'..='\u{061A}'
            | '\u{064B}'..='\u{065F}'
            | '\u{0670}'
            | '\u{06D6}'..='\u{06ED}'
            | '\u{0898}'..='\u{089F}'
            | '\u{08CA}'..='\u{08E1}'
            | '\u{08E3}'..='\u{08FF}'
            // Syriac, Thaana, N'Ko, Samaritan, Mandaic.
            | '\u{0711}'
            | '\u{0730}'..='\u{074A}'
            | '\u{07A6}'..='\u{07B0}'
            | '\u{07EB}'..='\u{07F3}'
            | '\u{07FD}'
            | '\u{0816}'..='\u{0819}'
            | '\u{081B}'..='\u{0823}'
            | '\u{0825}'..='\u{0827}'
            | '\u{0829}'..='\u{082D}'
            | '\u{0859}'..='\u{085B}'
            | '\u{1AB0}'..='\u{1AFF}'
            | '\u{20D0}'..='\u{20FF}'
            | '\u{FE20}'..='\u{FE2F}'
            // Hanifi Rohingya, Adlam.
            | '\u{10D24}'..='\u{10D27}'
            | '\u{1E944}'..='\u{1E94A}'
    )
}

fn piece_direction(piece: &str) -> Option<bool> {
    piece.chars().find_map(direction)
}

/// Whether more of the line's letters read right to left than left to right.
fn right_to_left(pieces: &[&str]) -> bool {
    let (mut right, mut left) = (0_usize, 0_usize);
    for ch in pieces.iter().flat_map(|piece| piece.chars()) {
        match direction(ch) {
            Some(true) => right += 1,
            Some(false) if ch.is_alphabetic() => left += 1,
            _ => {}
        }
    }
    right > left
}

/// Puts a right-to-left line, drawn left to right, in reading order: runs
/// of left-to-right text (Latin words, numbers) keep their order inside,
/// everything else reads from the right. Spaces and punctuation between two
/// left-to-right pieces belong to their run.
fn reorder(pieces: &mut [&str]) {
    let directions: Vec<Option<bool>> = pieces.iter().map(|piece| piece_direction(piece)).collect();
    let left_to_right: Vec<bool> = (0..pieces.len())
        .map(|at| {
            directions[at].map_or_else(
                || {
                    let before = directions[..at].iter().rev().find_map(|direction| *direction);
                    let after = directions[at + 1..].iter().find_map(|direction| *direction);
                    before == Some(false) && after == Some(false)
                },
                |rtl| !rtl,
            )
        })
        .collect();
    pieces.reverse();
    let mut at = 0;
    let total = pieces.len();
    while at < total {
        // Reversed, so a left-to-right run is now backwards: turn it back.
        if left_to_right[total - 1 - at] {
            let start = at;
            while at < total && left_to_right[total - 1 - at] {
                at += 1;
            }
            pieces[start..at].reverse();
        } else {
            at += 1;
        }
    }
}

/// The document's pages, from their lines: blocks of the lines that are
/// not running headers or footers.
pub fn pages(pages: &[PageLines]) -> Vec<Page> {
    let boilerplate = running_lines(pages);
    let kept = |line: &&Line| line.margin == 0 || boilerplate.binary_search(&line.margin).is_err();
    let body = body_size(pages, &kept);
    let levels = heading_sizes(pages, &kept, body);
    let mut out = Vec::with_capacity(pages.len());
    let mut paragraph: Vec<&Line> = Vec::new();
    // Each page's blocks, collected here, then moved out at their size.
    let mut blocks = Vec::new();
    for (number, page) in (1..).zip(pages) {
        paragraph.clear();
        for line in page.lines.iter().filter(kept) {
            if let Some(last) = paragraph.last()
                && !continues(last, line)
            {
                blocks.push(block(page, &paragraph, body, &levels));
                paragraph.clear();
            }
            paragraph.push(line);
        }
        if !paragraph.is_empty() {
            blocks.push(block(page, &paragraph, body, &levels));
        }
        #[expect(
            clippy::drain_collect,
            clippy::iter_with_drain,
            reason = "`blocks` keeps its buffer for the next page"
        )]
        let blocks = blocks.drain(..).collect();
        out.push(Page { number, blocks, images: page.images });
    }
    out
}

/// Whether `next` carries on the paragraph `last` is in: same direction,
/// similar size, and no more than a line and a half below it.
fn continues(last: &Line, next: &Line) -> bool {
    let size = last.size.max(next.size).max(1.0);
    last.turns == next.turns
        && (last.size - next.size).abs() <= 0.15 * size
        && last.y - next.y > 0.0
        && last.y - next.y <= 1.55 * size
        && last.bold == next.bold
}

/// A paragraph of lines as a block: a heading when its few short lines
/// are set in a heading size, otherwise a paragraph with its lines on lines
/// of their own, words hyphenated across them joined again.
fn block(page: &PageLines, lines: &[&Line], body: f64, levels: &[f64]) -> Placed {
    let first = lines[0];
    let length: usize = lines.iter().map(|line| line.span.len()).sum();
    let level = if lines.len() <= 3 && length <= 200 {
        levels.iter().position(|&size| first.size >= size - 0.25)
    } else {
        None
    };
    let mut text = String::with_capacity(length + lines.len());
    let block = if let Some(level) = level.filter(|_| first.size >= body * 1.15) {
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                text.push(' ');
            }
            text.push_str(page.text(line));
        }
        let level = u8::try_from(level.min(2) + 1).unwrap_or(3);
        Block::Heading { level, text: Text::plain(text) }
    } else {
        for (index, line) in lines.iter().enumerate() {
            let line = page.text(line);
            if index > 0 {
                let joined = text.ends_with('-') && {
                    let mut before = text.chars().rev().skip(1);
                    before.next().is_some_and(char::is_lowercase)
                        && line.chars().next().is_some_and(char::is_lowercase)
                };
                if joined {
                    text.pop();
                } else {
                    text.push('\n');
                }
            }
            text.push_str(line);
        }
        Block::Paragraph { text: Text::plain(text) }
    };
    Placed { block, bounds: bounds(page, lines) }
}

/// Where upright lines lie on the page, from its top-left corner: from the
/// top of the first line's capitals to below the last line's descenders.
fn bounds(page: &PageLines, lines: &[&Line]) -> Option<Bounds> {
    if lines.iter().any(|line| line.turns != 0) {
        return None;
    }
    let (first, last) = (lines.first()?, lines.last()?);
    let left = lines.iter().map(|line| line.left).fold(f64::MAX, f64::min);
    let right = lines.iter().map(|line| line.right).fold(f64::MIN, f64::max);
    let top = page.height - first.size.mul_add(0.8, first.y);
    let bottom = page.height - last.size.mul_add(-0.25, last.y);
    Some(Bounds { left, top, right, bottom })
}

/// Text that repeats in the margins of many pages (page numbers, running
/// titles), digits aside: their margin keys, sorted.
fn running_lines(pages: &[PageLines]) -> Vec<u64> {
    if pages.len() < 3 {
        return Vec::new();
    }
    let mut counts: HashMap<u64, usize> = HashMap::new();
    let mut seen: Vec<u64> = Vec::new();
    for page in pages {
        seen.clear();
        seen.extend(page.lines.iter().map(|line| line.margin).filter(|&key| key != 0));
        seen.sort_unstable();
        seen.dedup();
        for &key in &seen {
            *counts.entry(key).or_default() += 1;
        }
    }
    let threshold = (pages.len() * 2 / 5).max(3);
    let mut running: Vec<u64> =
        counts.into_iter().filter(|&(_, count)| count >= threshold).map(|(key, _)| key).collect();
    running.sort_unstable();
    running
}

/// Every page's lines that `kept` keeps.
fn kept_lines<'p>(
    pages: &'p [PageLines],
    kept: &'p impl Fn(&&Line) -> bool,
) -> impl Iterator<Item = &'p Line> {
    pages.iter().flat_map(|page| &page.lines).filter(kept)
}

/// The most common font size, by characters.
fn body_size(pages: &[PageLines], kept: &impl Fn(&&Line) -> bool) -> f64 {
    let mut counts: HashMap<i64, usize> = HashMap::new();
    for line in kept_lines(pages, kept) {
        #[expect(clippy::cast_possible_truncation, reason = "font sizes are small")]
        let key = (line.size * 2.0).round() as i64;
        *counts.entry(key).or_default() += line.span.len();
    }
    #[expect(clippy::cast_precision_loss, reason = "a small integer")]
    counts
        .into_iter()
        .max_by_key(|&(size, count)| (count, -size))
        .map_or(0.0, |(size, _)| size as f64 / 2.0)
}

/// The sizes of headings, largest first: distinct sizes clearly above the
/// body's, three at most.
fn heading_sizes(pages: &[PageLines], kept: &impl Fn(&&Line) -> bool, body: f64) -> Vec<f64> {
    let mut sizes: Vec<f64> = kept_lines(pages, kept)
        .filter(|line| line.size >= body * 1.15 && line.span.len() <= 200)
        .map(|line| (line.size * 2.0).round() / 2.0)
        .collect();
    sizes.sort_by(|a, b| b.total_cmp(a));
    sizes.dedup();
    sizes.truncate(3);
    sizes
}
