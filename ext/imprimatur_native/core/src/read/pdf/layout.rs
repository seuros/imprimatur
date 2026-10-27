//! From glyphs to blocks: lines by baseline, words by the gaps between
//! glyphs, paragraphs by the gaps between lines, headings by size, and
//! the running headers and footers left out.

mod bidi;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::ops::Range;

use self::bidi::{Scratch, is_mark};
use super::content::{Glyph, PageText};
use crate::read::content::{Block, Bounds, Page, Placed, Text};
use crate::read::span;
use crate::script::Counts;

/// Share of the page height, at the top and the bottom, where running
/// headers and footers live.
const EDGE: f64 = 0.09;
/// A gap wider than this share of the font size separates words.
const WORD_GAP: f64 = 0.18;
const POSITION_BUCKET: f64 = 3.0;

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
    /// A hash of the text and its bucketed position and direction, to spot
    /// anything else that repeats at the same spot on most pages.
    pub repeat: u64,
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
    let right_to_left = right_to_left(page);
    let mut pieces: Vec<&str> = Vec::new();
    let mut scratch = Scratch::default();
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
            let mut end = row_end(&glyphs, start);
            if let Some(merged) = attach_superscript(&glyphs, start, end) {
                end = merged;
            }
            let row = &mut glyphs[start..end];
            row.sort_by(|a, b| a.0.total_cmp(&b.0));
            let added = out.lines.len();
            line(page, row, turns, right_to_left, &mut pieces, &mut scratch, &mut out);
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

fn row_end(glyphs: &[(f64, f64, &Glyph)], start: usize) -> usize {
    let (_, baseline, first) = glyphs[start];
    let mut end = start + 1;
    while end < glyphs.len() {
        let (_, y, glyph) = glyphs[end];
        if (baseline - y).abs() > 0.45 * first.size.max(glyph.size) {
            break;
        }
        end += 1;
    }
    end
}

const SUPERSCRIPT_GLYPHS: usize = 3;
const SUPERSCRIPT_SIZE_RATIO: f64 = 0.85;

fn attach_superscript(glyphs: &[(f64, f64, &Glyph)], start: usize, end: usize) -> Option<usize> {
    if end - start > SUPERSCRIPT_GLYPHS || end >= glyphs.len() {
        return None;
    }
    let row = &glyphs[start..end];
    let (_, baseline, _) = glyphs[start];
    let (next_x, next_y, next) = glyphs[end];
    let base_size = next.size;
    let candidate_size = row.iter().map(|&(_, _, glyph)| glyph.size).fold(0.0, f64::max);
    let raised = baseline - next_y;
    if raised <= 0.0 || raised > base_size.max(candidate_size) {
        return None;
    }
    if row.iter().any(|&(_, _, glyph)| glyph.size > SUPERSCRIPT_SIZE_RATIO * base_size) {
        return None;
    }
    let row_left = row.iter().map(|&(x, _, _)| x).fold(f64::MAX, f64::min);
    let row_right = row.iter().map(|&(x, _, glyph)| x + glyph.width).fold(f64::MIN, f64::max);
    let next_right = next_x + next.width;
    let gap = if row_left >= next_right {
        row_left - next_right
    } else if next_x >= row_right {
        next_x - row_right
    } else {
        return None;
    };
    if gap > WORD_GAP * base_size.max(1.0) {
        return None;
    }
    Some(row_end(glyphs, end))
}

/// Whether a page reads right to left: more of its letters, those in the
/// top and bottom margins of upright text aside, are right to left.
fn right_to_left(page: &PageText) -> bool {
    let height = page.size.1.max(1.0);
    let mut counts = Counts::default();
    for glyph in &page.glyphs {
        if glyph.turns == 0 && (glyph.y > height * (1.0 - EDGE) || glyph.y < height * EDGE) {
            continue;
        }
        counts.add(&page.text[glyph.text.start as usize..glyph.text.end as usize]);
    }
    counts.right_to_left()
}

/// One line's text, appended to `out.text`: spaces where glyphs leave a
/// word gap, a glyph drawn twice on itself (fake bold) kept once, and
/// right-to-left text in reading order, in the page's direction. A line
/// with no text is not added.
fn line<'p>(
    page: &'p PageText,
    row: &[(f64, f64, &Glyph)],
    turns: u8,
    right_to_left: bool,
    pieces: &mut Vec<&'p str>,
    scratch: &mut Scratch,
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
    bidi::reading_order(pieces, right_to_left, scratch);
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
    let text = span::slice(&out.text, &span);
    let repeat = repeat_key(text, left, y, turns);
    out.lines.push(Line { span, y, left, right, size, bold, turns, margin: 0, repeat });
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

#[expect(clippy::cast_possible_truncation, reason = "page coordinates fit in i64")]
fn repeat_key(text: &str, left: f64, y: f64, turns: u8) -> u64 {
    let bucket = |value: f64| (value / POSITION_BUCKET).round() as i64;
    let mut hash = margin_key(text);
    for part in [u64::from(turns), bucket(left).cast_unsigned(), bucket(y).cast_unsigned()] {
        hash = (hash ^ part).wrapping_mul(0x0100_0000_01b3);
    }
    hash.max(1)
}

/// The document's pages, from their lines: blocks of the lines that are
/// not running headers or footers, and, with `drop_repeated_anywhere`, not
/// text repeated at the same spot on most pages anywhere on the page.
pub fn pages(
    pages: &[PageLines],
    labels: Option<&[Option<String>]>,
    drop_repeated_anywhere: bool,
) -> Vec<Page> {
    let boilerplate = repeated_keys(pages, |line| line.margin);
    let margin_kept =
        |line: &&Line| line.margin == 0 || boilerplate.binary_search(&line.margin).is_err();
    let repeated =
        if drop_repeated_anywhere { repeated_keys(pages, |line| line.repeat) } else { Vec::new() };
    let kept = |line: &&Line| {
        margin_kept(line) && (repeated.is_empty() || repeated.binary_search(&line.repeat).is_err())
    };
    let body = body_size(pages, &margin_kept);
    let levels = heading_sizes(pages, &margin_kept, body);
    let mut out = Vec::with_capacity(pages.len());
    let mut paragraph: Vec<&Line> = Vec::new();
    // Each page's blocks, collected here, then moved out at their size.
    let mut blocks = Vec::new();
    for (index, (number, page)) in (1..).zip(pages).enumerate() {
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
        let label = labels
            .map_or_else(|| printed_label(page), |labels| labels.get(index).cloned().flatten());
        out.push(Page { number, blocks, images: page.images, label });
    }
    out
}

/// The page's own printed label, from a single header or footer line that
/// looks like one: a bare page number or roman numeral ("`-i-`", "`7`"), or
/// the same after a short label word ("`Page 70`" -> "`70`"). Unlike the
/// boilerplate this document's other margin lines are checked against, a
/// running "`Page 70`", "`Page 71`", ... header is not noise here: each
/// instance *is* that page's label, not a repeated line to drop.
fn printed_label(page: &PageLines) -> Option<String> {
    let mut candidates =
        page.lines.iter().filter(|line: &&Line| line.margin != 0).filter_map(|line| {
            let text = page.text(line).trim();
            page_number_label(text)
        });
    let label = candidates.next()?;
    candidates.next().is_none().then_some(label)
}

/// `text` as a page label, if it looks like one, stripped of a leading
/// label word such as "Page" when there is one.
fn page_number_label(text: &str) -> Option<String> {
    if looks_like_page_number(text) {
        return Some(text.to_owned());
    }
    let word_end = text.find(|ch: char| !ch.is_ascii_alphabetic())?;
    if word_end == 0 {
        return None;
    }
    let rest = text[word_end..].trim_start_matches(|ch: char| !ch.is_alphanumeric());
    looks_like_page_number(rest).then(|| rest.to_owned())
}

fn looks_like_page_number(text: &str) -> bool {
    let core = text.trim_matches(|ch: char| !ch.is_alphanumeric());
    !core.is_empty()
        && core.len() <= 8
        && (core.chars().all(|ch| ch.is_ascii_digit())
            || core.chars().all(|ch| {
                matches!(ch.to_ascii_lowercase(), 'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm')
            }))
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
    let short = lines.len() <= 3 && length <= 200;
    let text = paragraph_text(page, lines);
    let level =
        if short { levels.iter().position(|&size| first.size >= size - 0.25) } else { None };
    let sized = level
        .filter(|_| first.size >= body * 1.15)
        .map(|level| u8::try_from(level.min(2) + 1).unwrap_or(3));
    let numbered = (short && first.bold && first.size >= body - 0.25)
        .then(|| numbered_heading_level(&text))
        .flatten();
    let block = match sized.or(numbered) {
        Some(level) => Block::Heading { level, text: Text::plain(text) },
        None => Block::Paragraph { text: Text::plain(text) },
    };
    Placed { block, bounds: bounds(page, lines) }
}

fn paragraph_text(page: &PageLines, lines: &[&Line]) -> String {
    let length: usize = lines.iter().map(|line| line.span.len()).sum();
    let mut text = String::with_capacity(length + lines.len());
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
    text
}

fn numbered_heading_level(text: &str) -> Option<u8> {
    let (depth, rest) = numbered_prefix(text)?;
    if depth < 2 || !(rest.is_empty() || rest.starts_with(' ')) {
        return None;
    }
    let title = rest.trim_start();
    if title.contains("....") || title.starts_with(|ch: char| !ch.is_alphanumeric()) {
        return None;
    }
    Some(u8::try_from(depth).unwrap_or(6).min(6))
}

fn numbered_prefix(text: &str) -> Option<(usize, &str)> {
    let bytes = text.as_bytes();
    let mut groups = 0_usize;
    let mut pos = 0_usize;
    loop {
        let start = pos;
        while bytes.get(pos).is_some_and(u8::is_ascii_digit) {
            pos += 1;
        }
        if pos == start {
            break;
        }
        groups += 1;
        if groups >= 4
            || bytes.get(pos) != Some(&b'.')
            || !bytes.get(pos + 1).is_some_and(u8::is_ascii_digit)
        {
            break;
        }
        pos += 1;
    }
    (groups > 0).then(|| (groups, &text[pos..]))
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

/// The keys `key` returns for lines that repeat on enough pages to be
/// boilerplate rather than coincidence, sorted.
fn repeated_keys(pages: &[PageLines], key: impl Fn(&Line) -> u64) -> Vec<u64> {
    if pages.len() < 3 {
        return Vec::new();
    }
    let mut counts: HashMap<u64, usize> = HashMap::new();
    let mut seen: Vec<u64> = Vec::new();
    for page in pages {
        seen.clear();
        seen.extend(page.lines.iter().map(&key).filter(|&key| key != 0));
        seen.sort_unstable();
        seen.dedup();
        for &key in &seen {
            *counts.entry(key).or_default() += 1;
        }
    }
    let threshold = (pages.len() * 2 / 5).max(3);
    let mut repeated: Vec<u64> =
        counts.into_iter().filter(|&(_, count)| count >= threshold).map(|(key, _)| key).collect();
    repeated.sort_unstable();
    repeated
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
